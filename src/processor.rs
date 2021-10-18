use std::{cell::Ref, mem::size_of};

use mango::{
    matching::{OrderType, Side},
    state::{
        MangoAccount, MangoCache, MangoGroup, RootBankCache, MAX_PAIRS, QUOTE_INDEX, ZERO_I80F48,
    },
};
use solana_program::{
    account_info::{next_account_info, Account, AccountInfo},
    entrypoint::ProgramResult,
    instruction::{AccountMeta, Instruction},
    msg,
    native_token::LAMPORTS_PER_SOL,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    program_pack::{IsInitialized, Pack},
    pubkey::Pubkey,
    system_instruction, system_program,
    sysvar::{rent::Rent, Sysvar},
};
use spl_associated_token_account::{create_associated_token_account, get_associated_token_address};
use spl_token::state::{Account as TokenAccount, Mint};

use mango_common::Loadable;
use mango_macro::{Loadable, Pod};

use arrayref::{array_ref, array_refs};
use fixed::types::I80F48;
use std::cell::RefMut;

use crate::{
    error::{check_assert, QuasarError, QuasarErrorCode, QuasarResult, SourceFileId},
    instruction::QuasarInstruction,
    oracle::{determine_oracle_type, OracleType, Price, StubOracle},
    state::{BaseToken, DataType, LeverageToken, MetaData, QuasarGroup, LEVERGAE_TOKEN_DECIMALS},
    utils::{gen_signer_key, gen_signer_seeds, get_mango_spot_value},
};

declare_check_assert_macros!(SourceFileId::Processor);
pub struct Processor;

impl Processor {
    pub fn process(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        instruction_data: &[u8],
    ) -> QuasarResult<()> {
        let instruction = QuasarInstruction::unpack(instruction_data)
            .ok_or(ProgramError::InvalidInstructionData)?;

        match instruction {
            QuasarInstruction::InitQuasarGroup { signer_nonce } => {
                msg!("Instruction: InitQuasarGroup");
                Self::init_quasar_group(program_id, accounts, signer_nonce)
            }
            QuasarInstruction::AddBaseToken => {
                msg!("Instruction: AddBaseToken");
                Self::add_base_token(program_id, accounts)
            }
            QuasarInstruction::AddLeverageToken { target_leverage } => {
                msg!("Instruction: AddLeverageToken");
                Self::add_leverage_token(program_id, accounts, target_leverage)
            }
            QuasarInstruction::MintLeverageToken { quantity } => {
                msg!("Instruction: MintLeverageToken");
                Self::mint_leverage_token(program_id, accounts, quantity)
            }
            QuasarInstruction::BurnLeverageToken { quantity } => {
                msg!("Instruction: BurnLeverageToken");
                Self::burn_leverage_token(program_id, accounts, quantity)
            }
            QuasarInstruction::Rebalance => {
                msg!("Instruction: Rebalance");
                Self::rebalance(program_id, accounts)
            }
        }
    }

    #[inline(never)]
    fn init_quasar_group(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        signer_nonce: u64,
    ) -> QuasarResult {
        const NUM_FIXED: usize = 4;
        let accounts = array_ref![accounts, 0, NUM_FIXED];

        let [quasar_group_ai, signer_ai, admin_ai, mango_program_ai] = accounts;
        check_eq!(
            quasar_group_ai.owner,
            program_id,
            QuasarErrorCode::InvalidGroupOwner
        )?;
        let rent = Rent::get()?;
        check!(
            rent.is_exempt(quasar_group_ai.lamports(), size_of::<QuasarGroup>()),
            QuasarErrorCode::GroupNotRentExempt
        )?;
        let mut quasar_group: RefMut<QuasarGroup> = QuasarGroup::load_mut(quasar_group_ai)?;
        check!(
            !quasar_group.meta_data.is_initialized,
            QuasarErrorCode::Default
        )?;

        check!(
            gen_signer_key(signer_nonce, quasar_group_ai.key, program_id)? == *signer_ai.key,
            QuasarErrorCode::InvalidSignerKey
        )?;
        quasar_group.signer_nonce = signer_nonce;
        quasar_group.signer_key = *signer_ai.key;
        quasar_group.mango_program_id = *mango_program_ai.key;

        check!(admin_ai.is_signer, QuasarErrorCode::Default)?;
        quasar_group.admin_key = *admin_ai.key;

        quasar_group.meta_data = MetaData::new(DataType::QuasarGroup, 0, true);

        Ok(())
    }

    #[inline(never)]
    fn add_base_token<'a>(program_id: &Pubkey, accounts: &[AccountInfo<'a>]) -> QuasarResult {
        const NUM_FIXED: usize = 4;
        let accounts = array_ref![accounts, 0, NUM_FIXED];

        let [quasar_group_ai, mint_ai, oracle_ai, admin_ai] = accounts;

        let mut quasar_group = QuasarGroup::load_mut_checked(quasar_group_ai, program_id)?;
        check!(admin_ai.is_signer, QuasarErrorCode::InvalidSignerKey)?;
        check_eq!(
            admin_ai.key,
            &quasar_group.admin_key,
            QuasarErrorCode::InvalidSignerKey
        )?;

        // Make sure there is no duplicated base token which has the same mint key
        check!(
            quasar_group.find_base_token_index(mint_ai.key).is_none(),
            QuasarErrorCode::Default
        )?;

        let oracle_type = determine_oracle_type(oracle_ai);
        match oracle_type {
            OracleType::Pyth => {
                msg!("OracleType:Pyth"); // Do nothing really cause all that's needed is storing the pkey
            }
            OracleType::Stub | OracleType::Unknown => {
                msg!("OracleType: got unknown or stub");
                let rent = Rent::get()?;
                let mut oracle = StubOracle::load_and_init(oracle_ai, program_id, &rent)?;
                oracle.magic = 0x6F676E4D;
            }
        }

        let base_token_index = quasar_group.num_base_tokens;
        // Make sure base token at this index is not already initialized
        check!(
            quasar_group.base_tokens[base_token_index].is_empty(),
            QuasarErrorCode::Default
        )?;

        let mint = Mint::unpack(&mint_ai.try_borrow_data()?)?;
        quasar_group.base_tokens[base_token_index] = BaseToken {
            mint: *mint_ai.key,
            decimals: mint.decimals,
            oracle: *oracle_ai.key,
            padding: [0u8; 7],
        };
        quasar_group.num_base_tokens += 1;

        Ok(())
    }

    #[inline(never)]
    /// Add a leveraged token to quasar group
    /// Only allow admin
    fn add_leverage_token(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        target_leverage: I80F48,
    ) -> QuasarResult {
        const NUM_FIXED: usize = 12;
        let accounts = array_ref![accounts, 0, NUM_FIXED];
        let [quasar_group_ai, mint_ai, base_token_mint_ai, mango_program_ai, mango_group_ai, mango_account_ai, mango_perp_market_ai, system_program_ai, token_program_ai, rent_program_ai, admin_ai, pda_ai] =
            accounts;

        let mut quasar_group = QuasarGroup::load_mut_checked(quasar_group_ai, program_id)?;
        check!(admin_ai.is_signer, QuasarErrorCode::SignerNecessary)?;
        check_eq!(
            admin_ai.key,
            &quasar_group.admin_key,
            QuasarErrorCode::InvalidAdminKey
        )?;

        // Make sure leverage token is referencing a proper base token
        check!(
            quasar_group
                .find_base_token_index(base_token_mint_ai.key)
                .is_some(),
            QuasarErrorCode::InvalidAccount
        )?;

        // Make sure there is no duplicated leverage token which has the same base token and the leverage target
        check!(
            quasar_group
                .find_leverage_token_index(base_token_mint_ai.key, target_leverage)
                .is_none(),
            QuasarErrorCode::Default
        )?;

        let token_index = quasar_group.num_leverage_tokens;

        // Make sure leverage token at this index is not already initialized
        check!(
            quasar_group.leverage_tokens[token_index].is_empty(),
            QuasarErrorCode::Default
        )?;

        check_eq!(
            *pda_ai.key,
            quasar_group.signer_key,
            QuasarErrorCode::InvalidSignerKey
        )?;
        let signer_seeds = gen_signer_seeds(&quasar_group.signer_nonce, quasar_group_ai.key);

        init_mango_account(
            mango_program_ai,
            mango_group_ai,
            mango_account_ai,
            pda_ai,
            &[&signer_seeds],
        )?;
        msg!("Init Mango Account succeeded");

        create_and_initialize_mint_account(
            admin_ai,
            mint_ai,
            pda_ai,
            token_program_ai,
            system_program_ai,
            rent_program_ai,
            &[&signer_seeds],
            LEVERGAE_TOKEN_DECIMALS,
        )?;
        msg!("target leverage: {}", target_leverage);

        quasar_group.leverage_tokens[token_index] = LeverageToken {
            mint: *mint_ai.key,
            base_token_mint: *base_token_mint_ai.key,
            target_leverage: target_leverage,
            mango_account: *mango_account_ai.key,
            mango_perp_market: *mango_perp_market_ai.key,
        };
        quasar_group.num_leverage_tokens += 1;

        Ok(())
    }

    #[inline(never)]
    fn mint_leverage_token<'a>(
        program_id: &Pubkey,
        accounts: &[AccountInfo<'a>],
        quantity: u64,
    ) -> QuasarResult {
        const NUM_FIXED: usize = 14;
        let accounts = array_ref![accounts, 0, NUM_FIXED];
        let [quasar_group_ai, token_mint_ai, owner_leverage_token_account_ai, mango_program_ai, mango_group_ai, mango_account_ai, owner_ai, mango_cache_ai, root_bank_ai, node_bank_ai, vault_ai, token_program_ai, owner_quote_token_account_ai, pda_ai] =
            accounts;

        let quasar_group = QuasarGroup::load_checked(quasar_group_ai, program_id)?;

        let native_price;
        {
            let mango_group = MangoGroup::load_checked(&mango_group_ai, mango_program_ai.key)?;
            let mango_cache =
                MangoCache::load_checked(&mango_cache_ai, mango_program_ai.key, &mango_group)?;
            let mango_account = MangoAccount::load_checked(
                &mango_account_ai,
                mango_program_ai.key,
                mango_group_ai.key,
            )?;

            check_eq!(
                *owner_leverage_token_account_ai.key,
                get_associated_token_address(owner_ai.key, token_mint_ai.key),
                QuasarErrorCode::InvalidAccount
            );

            let leverage_token_index = quasar_group
                .find_leverage_token_index_by_mint(token_mint_ai.key)
                .unwrap();
            let leverage_token = quasar_group.leverage_tokens[leverage_token_index];

            check_eq!(
                leverage_token.mango_account,
                *mango_account_ai.key,
                QuasarErrorCode::InvalidAccount
            );

            native_price = leverage_token.get_native_price(
                token_mint_ai,
                &mango_group,
                &mango_account,
                &mango_cache,
            )?;
        }

        deposit_to_mango_account(
            mango_program_ai,
            mango_group_ai,
            mango_account_ai,
            owner_ai,
            mango_cache_ai,
            root_bank_ai,
            node_bank_ai,
            vault_ai,
            token_program_ai,
            owner_quote_token_account_ai,
            &[&[]],
            quantity * native_price.to_num::<u64>(),
        )?;

        let signer_seeds = gen_signer_seeds(&quasar_group.signer_nonce, quasar_group_ai.key);
        invoke_mint_to(
            token_program_ai,
            token_mint_ai,
            owner_leverage_token_account_ai,
            pda_ai,
            &[&signer_seeds],
            quantity,
        )?;

        Ok(())
    }

    #[inline(never)]
    fn burn_leverage_token<'a>(
        program_id: &Pubkey,
        accounts: &[AccountInfo<'a>],
        quantity: u64,
    ) -> QuasarResult {
        const NUM_FIXED: usize = 15;
        let accounts = array_ref![accounts, 0, NUM_FIXED + MAX_PAIRS];
        let (fixed_ais, mango_open_orders_ais) = array_refs![accounts, NUM_FIXED, MAX_PAIRS];
        let [quasar_group_ai, token_mint_ai, owner_leverage_token_account_ai, mango_program_ai, mango_group_ai, mango_account_ai, owner_ai, mango_cache_ai, root_bank_ai, node_bank_ai, vault_ai, token_program_ai, owner_quote_token_account_ai, pda_ai, mango_signer_ai] =
            fixed_ais;

        let quasar_group = QuasarGroup::load_checked(quasar_group_ai, program_id)?;

        check_eq!(
            *owner_leverage_token_account_ai.key,
            get_associated_token_address(owner_ai.key, token_mint_ai.key),
            QuasarErrorCode::InvalidAccount
        );

        let leverage_token_index =
            quasar_group.find_leverage_token_index_by_mint(token_mint_ai.key);
        check!(
            leverage_token_index.is_some(),
            QuasarErrorCode::InvalidToken
        );

        let native_price;
        {
            let mango_group = MangoGroup::load_checked(&mango_group_ai, mango_program_ai.key)?;
            let mango_cache =
                MangoCache::load_checked(&mango_cache_ai, mango_program_ai.key, &mango_group)?;
            let mango_account = MangoAccount::load_checked(
                &mango_account_ai,
                mango_program_ai.key,
                mango_group_ai.key,
            )?;

            check_eq!(
                *owner_leverage_token_account_ai.key,
                get_associated_token_address(owner_ai.key, token_mint_ai.key),
                QuasarErrorCode::InvalidAccount
            );

            let leverage_token_index = quasar_group
                .find_leverage_token_index_by_mint(token_mint_ai.key)
                .unwrap();
            let leverage_token = quasar_group.leverage_tokens[leverage_token_index];

            check_eq!(
                leverage_token.mango_account,
                *mango_account_ai.key,
                QuasarErrorCode::InvalidAccount
            );

            native_price = leverage_token.get_native_price(
                token_mint_ai,
                &mango_group,
                &mango_account,
                &mango_cache,
            )?;
        }

        invoke_burn(
            token_program_ai,
            owner_leverage_token_account_ai,
            token_mint_ai,
            owner_ai,
            &[],
            quantity,
        )?;

        let signer_seeds = gen_signer_seeds(&quasar_group.signer_nonce, quasar_group_ai.key);

        withdraw_from_mango_account(
            mango_program_ai,
            mango_group_ai,
            mango_account_ai,
            pda_ai,
            mango_cache_ai,
            root_bank_ai,
            node_bank_ai,
            vault_ai,
            owner_quote_token_account_ai,
            mango_signer_ai,
            token_program_ai,
            mango_open_orders_ais,
            &[&signer_seeds],
            quantity * native_price.to_num::<u64>(),
            false,
        )?;

        Ok(())
    }

    #[inline(never)]
    fn rebalance<'a>(program_id: &Pubkey, accounts: &[AccountInfo<'a>]) -> QuasarResult {
        const NUM_FIXED: usize = 12;
        let accounts = array_ref![accounts, 0, NUM_FIXED + MAX_PAIRS];
        let (fixed_ais, mango_open_orders_ais) = array_refs![accounts, NUM_FIXED, MAX_PAIRS];
        let [quasar_group_ai, token_mint_ai, pda_ai, mango_program_ai, mango_group_ai, mango_account_ai, owner_ai, mango_cache_ai, mango_perp_market_ai, mango_bids_ai, mango_asks_ai, mango_event_queue_ai] =
            fixed_ais;

        let quasar_group = QuasarGroup::load_checked(quasar_group_ai, program_id)?;

        let leverage_token_index = quasar_group
            .find_leverage_token_index_by_mint(token_mint_ai.key)
            .unwrap();
        let leverage_token = quasar_group.leverage_tokens[leverage_token_index];

        check_eq!(
            leverage_token.mango_account,
            *mango_account_ai.key,
            QuasarErrorCode::InvalidAccount
        );
        check_eq!(
            leverage_token.mango_perp_market,
            *mango_perp_market_ai.key,
            QuasarErrorCode::InvalidAccount
        );

        let mut price;
        let mut quantity;
        {
            let mango_group = MangoGroup::load_checked(&mango_group_ai, mango_program_ai.key)?;
            let mango_cache =
                MangoCache::load_checked(&mango_cache_ai, mango_program_ai.key, &mango_group)?;

            let mango_account = MangoAccount::load_checked(
                &mango_account_ai,
                mango_program_ai.key,
                mango_group_ai.key,
            )?;

            let mut net_asset_value = ZERO_I80F48;
            let mut perp_asset_value = ZERO_I80F48;

            let market_index = mango_group
                .find_perp_market_index(&leverage_token.mango_perp_market)
                .unwrap();

            for i in 0..mango_group.num_oracles {
                let spot_value = get_mango_spot_value(
                    &mango_account,
                    &mango_cache.root_bank_cache[i],
                    mango_cache.price_cache[i].price,
                    i,
                )?;

                let (perp_base_value, perp_quote_value) = mango_account.perp_accounts[i].get_val(
                    &mango_group.perp_markets[i],
                    &mango_cache.perp_market_cache[i],
                    mango_cache.price_cache[i].price,
                )?;

                msg!(
                    "market {}: spot {} / perp_base {} / perp_quote {}",
                    i,
                    spot_value,
                    perp_base_value,
                    perp_quote_value,
                );

                net_asset_value = net_asset_value
                    .checked_add(
                        spot_value
                            .checked_add(perp_base_value.checked_add(perp_quote_value).unwrap())
                            .unwrap(),
                    )
                    .unwrap();

                perp_asset_value = perp_asset_value.checked_add(perp_base_value).unwrap();
            }

            msg!("net asset value: {}", net_asset_value);
            msg!("perp asset value: {}", perp_asset_value);
            msg!("effective leverage: {}", perp_asset_value / net_asset_value);

            price = mango_cache.price_cache[market_index].price;
            msg!("price: {}", price);
            let target_exposure = net_asset_value
                .checked_mul(leverage_token.target_leverage)
                .unwrap();
            msg!("target leverage: {}", leverage_token.target_leverage);
            msg!("target exposure: {}", target_exposure);
            msg!("current exposure: {}", perp_asset_value);

            let base_decimals = mango_group.tokens[market_index].decimals;
            let base_unit = 10u64.pow(base_decimals.into());
            let base_lot_size =
                I80F48::from_num(mango_group.perp_markets[market_index].base_lot_size);

            let quote_decimals = mango_group.tokens[QUOTE_INDEX].decimals;
            let quote_unit = 10u64.pow(quote_decimals.into());
            let quote_lot_size =
                I80F48::from_num(mango_group.perp_markets[market_index].quote_lot_size);

            let exposure_delta = target_exposure.checked_sub(perp_asset_value).unwrap();
            msg!("exposure delta in native quote unit: {}", exposure_delta);

            price = price
                .checked_mul(I80F48::from_num(quote_unit))
                .unwrap()
                .checked_mul(base_lot_size)
                .unwrap()
                .checked_div(quote_lot_size)
                .unwrap()
                .checked_div(I80F48::from_num(base_unit))
                .unwrap();
            msg!("price in quote lot unit: {}", price);

            let exposure_delta = exposure_delta
                .checked_div(I80F48::from_num(quote_lot_size))
                .unwrap();
            msg!("exposure delta in quote lot unit: {}", exposure_delta);

            quantity = exposure_delta.checked_div(price).unwrap();
            msg!("perp quantity to adjust in base lot unit: {}", quantity);
        }

        let signer_seeds = gen_signer_seeds(&quasar_group.signer_nonce, quasar_group_ai.key);

        let price = price.to_num::<i64>();
        let quantity = quantity.to_num::<i64>();
        msg!("price: {}, quantity: {}", price, quantity.abs());

        if quantity.abs() > 0 {
            place_mango_perp_order(
                mango_program_ai,
                mango_group_ai,
                mango_account_ai,
                pda_ai,
                mango_cache_ai,
                mango_perp_market_ai,
                mango_bids_ai,
                mango_asks_ai,
                mango_event_queue_ai,
                mango_open_orders_ais,
                &[&signer_seeds],
                price,
                quantity.abs(),
                0,
                if quantity > ZERO_I80F48 {
                    Side::Bid
                } else {
                    Side::Ask
                },
                OrderType::Limit,
            )?;
        }

        Ok(())
    }
}

fn create_account<'a>(
    signer_ai: &AccountInfo<'a>,
    new_account_ai: &AccountInfo<'a>,
    space: usize,
    owner_ai: &AccountInfo<'a>,
    system_program_ai: &AccountInfo<'a>,
) -> ProgramResult {
    let rent = Rent::default().minimum_balance(space);

    check_eq!(
        *system_program_ai.key,
        solana_program::system_program::id(),
        QuasarErrorCode::InvalidAccount
    )?;

    let instruction = solana_program::system_instruction::create_account(
        signer_ai.key,
        new_account_ai.key,
        rent,
        space as u64,
        owner_ai.key,
    );

    let account_infos = [
        system_program_ai.clone(),
        signer_ai.clone(),
        new_account_ai.clone(),
    ];

    invoke(&instruction, &account_infos)
}

fn invoke_mint_to<'a>(
    token_program_ai: &AccountInfo<'a>,
    mint_ai: &AccountInfo<'a>,
    account_ai: &AccountInfo<'a>,
    owner_ai: &AccountInfo<'a>,
    signer_seeds: &[&[&[u8]]],
    quantity: u64,
) -> ProgramResult {
    let instruction = spl_token::instruction::mint_to(
        &spl_token::ID,
        mint_ai.key,
        account_ai.key,
        owner_ai.key,
        &[],
        quantity,
    )?;

    let account_infos = [
        token_program_ai.clone(),
        mint_ai.clone(),
        account_ai.clone(),
        owner_ai.clone(),
    ];

    solana_program::program::invoke_signed(&instruction, &account_infos, signer_seeds)
}

fn invoke_burn<'a>(
    token_program_ai: &AccountInfo<'a>,
    account_ai: &AccountInfo<'a>,
    mint_ai: &AccountInfo<'a>,
    owner_ai: &AccountInfo<'a>,
    signer_seeds: &[&[&[u8]]],
    quantity: u64,
) -> ProgramResult {
    let instruction = spl_token::instruction::burn(
        &spl_token::ID,
        account_ai.key,
        mint_ai.key,
        owner_ai.key,
        &[],
        quantity,
    )?;

    let account_infos = [
        token_program_ai.clone(),
        mint_ai.clone(),
        account_ai.clone(),
        owner_ai.clone(),
    ];

    solana_program::program::invoke_signed(&instruction, &account_infos, signer_seeds)
}

fn init_mango_account<'a>(
    mango_program_ai: &AccountInfo<'a>,
    mango_group_ai: &AccountInfo<'a>,
    mango_account_ai: &AccountInfo<'a>,
    owner_ai: &AccountInfo<'a>,
    signers_seeds: &[&[&[u8]]],
) -> ProgramResult {
    let instruction = Instruction {
        program_id: *mango_program_ai.key,
        data: mango::instruction::MangoInstruction::InitMangoAccount.pack(),
        accounts: vec![
            AccountMeta::new_readonly(*mango_group_ai.key, false),
            AccountMeta::new(*mango_account_ai.key, false),
            AccountMeta::new_readonly(*owner_ai.key, true),
        ],
    };

    let account_infos = [
        mango_program_ai.clone(),
        mango_group_ai.clone(),
        mango_account_ai.clone(),
        owner_ai.clone(),
    ];

    invoke_signed(&instruction, &account_infos, signers_seeds)
}

fn deposit_to_mango_account<'a>(
    mango_program_ai: &AccountInfo<'a>,
    mango_group_ai: &AccountInfo<'a>,
    mango_account_ai: &AccountInfo<'a>,
    owner_ai: &AccountInfo<'a>,
    mango_cache_ai: &AccountInfo<'a>,
    root_bank_ai: &AccountInfo<'a>,
    node_bank_ai: &AccountInfo<'a>,
    vault_ai: &AccountInfo<'a>,
    token_program_ai: &AccountInfo<'a>,
    owner_token_account_ai: &AccountInfo<'a>,
    signers_seeds: &[&[&[u8]]],
    quantity: u64,
) -> ProgramResult {
    let instruction = Instruction {
        program_id: *mango_program_ai.key,
        data: mango::instruction::MangoInstruction::Deposit { quantity }.pack(),
        accounts: vec![
            AccountMeta::new_readonly(*mango_group_ai.key, false),
            AccountMeta::new(*mango_account_ai.key, false),
            AccountMeta::new_readonly(*owner_ai.key, true),
            AccountMeta::new_readonly(*mango_cache_ai.key, false),
            AccountMeta::new_readonly(*root_bank_ai.key, false),
            AccountMeta::new(*node_bank_ai.key, false),
            AccountMeta::new(*vault_ai.key, false),
            AccountMeta::new_readonly(*token_program_ai.key, false),
            AccountMeta::new(*owner_token_account_ai.key, false),
        ],
    };

    let account_infos = [
        mango_program_ai.clone(),
        mango_group_ai.clone(),
        mango_account_ai.clone(),
        owner_ai.clone(),
        mango_cache_ai.clone(),
        root_bank_ai.clone(),
        node_bank_ai.clone(),
        vault_ai.clone(),
        token_program_ai.clone(),
        owner_token_account_ai.clone(),
    ];

    invoke_signed(&instruction, &account_infos, signers_seeds)
}

fn withdraw_from_mango_account<'a>(
    mango_program_ai: &AccountInfo<'a>,
    mango_group_ai: &AccountInfo<'a>,
    mango_account_ai: &AccountInfo<'a>,
    owner_ai: &AccountInfo<'a>,
    mango_cache_ai: &AccountInfo<'a>,
    root_bank_ai: &AccountInfo<'a>,
    node_bank_ai: &AccountInfo<'a>,
    vault_ai: &AccountInfo<'a>,
    owner_token_account_ai: &AccountInfo<'a>,
    signer_ai: &AccountInfo<'a>,
    token_program_ai: &AccountInfo<'a>,
    mango_open_orders_ais: &[AccountInfo<'a>; MAX_PAIRS],
    signers_seeds: &[&[&[u8]]],
    quantity: u64,
    allow_borrow: bool,
) -> ProgramResult {
    let mut accounts = vec![
        AccountMeta::new_readonly(*mango_group_ai.key, false),
        AccountMeta::new(*mango_account_ai.key, false),
        AccountMeta::new_readonly(*owner_ai.key, true),
        AccountMeta::new_readonly(*mango_cache_ai.key, false),
        AccountMeta::new_readonly(*root_bank_ai.key, false),
        AccountMeta::new(*node_bank_ai.key, false),
        AccountMeta::new(*vault_ai.key, false),
        AccountMeta::new(*owner_token_account_ai.key, false),
        AccountMeta::new_readonly(*signer_ai.key, false),
        AccountMeta::new_readonly(*token_program_ai.key, false),
    ];
    accounts.extend(
        mango_open_orders_ais
            .iter()
            .map(|ai| AccountMeta::new_readonly(*ai.key, false)),
    );

    let instruction = Instruction {
        program_id: *mango_program_ai.key,
        data: mango::instruction::MangoInstruction::Withdraw {
            quantity,
            allow_borrow,
        }
        .pack(),
        accounts: accounts,
    };

    let mut account_infos = [
        mango_program_ai.clone(),
        mango_group_ai.clone(),
        mango_account_ai.clone(),
        owner_ai.clone(),
        mango_cache_ai.clone(),
        root_bank_ai.clone(),
        node_bank_ai.clone(),
        vault_ai.clone(),
        owner_token_account_ai.clone(),
        signer_ai.clone(),
        token_program_ai.clone(),
    ]
    .to_vec();
    account_infos.extend(mango_open_orders_ais.iter().map(|ai| ai.clone()));
    let account_infos = account_infos.as_slice();

    invoke_signed(&instruction, account_infos, signers_seeds)
}

fn place_mango_perp_order<'a>(
    mango_program_ai: &AccountInfo<'a>,
    mango_group_ai: &AccountInfo<'a>,
    mango_account_ai: &AccountInfo<'a>,
    owner_ai: &AccountInfo<'a>,
    mango_cache_ai: &AccountInfo<'a>,
    mango_perp_market_ai: &AccountInfo<'a>,
    mango_bids_ai: &AccountInfo<'a>,
    mango_asks_ai: &AccountInfo<'a>,
    mango_event_queue_ai: &AccountInfo<'a>,
    mango_open_orders_ais: &[AccountInfo<'a>; MAX_PAIRS],
    signers_seeds: &[&[&[u8]]],
    price: i64,
    quantity: i64,
    client_order_id: u64,
    side: Side,
    order_type: OrderType,
) -> ProgramResult {
    let mut accounts = vec![
        AccountMeta::new_readonly(*mango_group_ai.key, false),
        AccountMeta::new(*mango_account_ai.key, false),
        AccountMeta::new_readonly(*owner_ai.key, true),
        AccountMeta::new_readonly(*mango_cache_ai.key, false),
        AccountMeta::new(*mango_perp_market_ai.key, false),
        AccountMeta::new(*mango_bids_ai.key, false),
        AccountMeta::new(*mango_asks_ai.key, false),
        AccountMeta::new(*mango_event_queue_ai.key, false),
    ];
    accounts.extend(
        mango_open_orders_ais
            .iter()
            .map(|ai| AccountMeta::new_readonly(*ai.key, false)),
    );

    let mut account_infos = [
        mango_program_ai.clone(),
        mango_group_ai.clone(),
        mango_account_ai.clone(),
        owner_ai.clone(),
        mango_cache_ai.clone(),
        mango_perp_market_ai.clone(),
        mango_bids_ai.clone(),
        mango_asks_ai.clone(),
        mango_event_queue_ai.clone(),
    ]
    .to_vec();
    account_infos.extend(mango_open_orders_ais.iter().map(|ai| ai.clone()));
    let account_infos = account_infos.as_slice();

    let instruction = Instruction {
        program_id: *mango_program_ai.key,
        data: mango::instruction::MangoInstruction::PlacePerpOrder {
            price,
            quantity,
            client_order_id,
            side,
            order_type,
        }
        .pack(),
        accounts: accounts,
    };

    invoke_signed(&instruction, &account_infos, signers_seeds)
}

fn create_and_initialize_mint_account<'a>(
    signer_ai: &AccountInfo<'a>,
    mint_ai: &AccountInfo<'a>,      // write
    authority_ai: &AccountInfo<'a>, // write
    token_program_ai: &AccountInfo<'a>,
    system_program_ai: &AccountInfo<'a>,
    rent_program_ai: &AccountInfo<'a>,
    signer_seeds: &[&[&[u8]]],
    decimals: u8,
) -> QuasarResult {
    check_eq!(
        *token_program_ai.key,
        spl_token::id(),
        QuasarErrorCode::InvalidAccount
    )?;

    check_eq!(
        *system_program_ai.key,
        solana_program::system_program::id(),
        QuasarErrorCode::InvalidAccount
    )?;

    check_eq!(
        *rent_program_ai.key,
        solana_program::sysvar::rent::id(),
        QuasarErrorCode::InvalidAccount
    )?;

    create_account(
        &signer_ai,
        mint_ai,
        Mint::LEN,
        &token_program_ai,
        &system_program_ai,
    )?;

    msg!("mint account {} created", mint_ai.key.to_string());

    let instruction = spl_token::instruction::initialize_mint(
        token_program_ai.key,
        mint_ai.key,
        authority_ai.key,
        Some(authority_ai.key),
        decimals,
    )?;

    solana_program::program::invoke_signed(
        &instruction,
        &[
            mint_ai.clone(),
            token_program_ai.clone(),
            rent_program_ai.clone(),
        ],
        signer_seeds,
    )?;

    Ok(())
}

#[inline(never)]
fn read_oracle(base_token: &BaseToken, oracle_ai: &AccountInfo) -> QuasarResult<I80F48> {
    let quote_decimals: u8 = base_token.decimals;
    let oracle_type = determine_oracle_type(oracle_ai);
    let price = match oracle_type {
        OracleType::Pyth => {
            let price_account = Price::get_price(oracle_ai).unwrap();
            let value = I80F48::from_num(price_account.agg.price);

            let decimals = (quote_decimals as i32)
                .checked_add(price_account.expo)
                .unwrap()
                .checked_sub(quote_decimals as i32)
                .unwrap();

            let decimal_adj = I80F48::from_num(10u64.pow(decimals.abs() as u32));
            if decimals < 0 {
                value.checked_div(decimal_adj).unwrap()
            } else {
                value.checked_mul(decimal_adj).unwrap()
            }
        }
        OracleType::Stub => {
            let oracle = StubOracle::load(oracle_ai)?;
            I80F48::from_num(oracle.price)
        }
        OracleType::Unknown => {
            panic!("Unknown oracle");
        }
    };
    Ok(price)
}
