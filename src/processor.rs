use std::mem::size_of;

use solana_program::{
    account_info::{next_account_info, Account, AccountInfo},
    entrypoint::ProgramResult,
    instruction::{AccountMeta, Instruction},
    msg,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    program_pack::{IsInitialized, Pack},
    pubkey::Pubkey,
    system_instruction, system_program,
    sysvar::{rent::Rent, Sysvar},
};
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
            QuasarInstruction::InitQuasarGroup => {
                msg!("Instruction: InitQuasarGroup");
                Self::init_quasar_group(program_id, accounts)
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
            QuasarInstruction::RedeemLeverageToken { quantity } => {
                msg!("Instruction: RedeemLeverageToken");
                Self::redeem_leverage_token(program_id, accounts, quantity)
            }
            QuasarInstruction::TestCreateAccount => {
                msg!("Instruction: TestCreateAccount");
                Self::test_create_account(program_id, accounts)
            }
            QuasarInstruction::TestInitializeMint => {
                msg!("Instruction: TestInitializeMint");
                Self::test_initialize_mint(program_id, accounts)
            }
        }
    }

    #[inline(never)]
    fn init_quasar_group(program_id: &Pubkey, accounts: &[AccountInfo]) -> QuasarResult {
        const NUM_FIXED: usize = 4;
        let accounts = array_ref![accounts, 0, NUM_FIXED];

        let [
            quasar_group_ai,     // write
            signer_ai,          // read
            admin_ai,           // read
            mango_program_ai         // read
        ] = accounts;
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

        let [
            quasar_group_ai,     // write
            mint_ai,         //read
            oracle_ai,          // read
            admin_ai,           // read
        ] = accounts;

        let mut quasar_group = QuasarGroup::load_mut_checked(quasar_group_ai, program_id)?;
        check!(admin_ai.is_signer, QuasarErrorCode::InvalidSignerKey)?;
        check_eq!(
            admin_ai.key,
            &quasar_group.admin_key,
            QuasarErrorCode::InvalidSignerKey
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
        // Make sure base token at this index not already initialized
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
        const NUM_FIXED: usize = 11;
        let accounts = array_ref![accounts, 0, NUM_FIXED];
        let [
            quasar_group_ai, // write
            mint_ai,        // read
            base_token_mint_ai,        // read
            mango_program_ai,
            mango_group_ai,
            mango_account_ai, // read
            mango_perp_market_ai,
            system_program_ai,
            token_program_ai,
            rent_program_ai,
            admin_ai        // read, signer
        ] = accounts;

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

        // Make sure there is no duplicated leverage token which has same base token and leverage target
        check!(
            quasar_group
                .find_leverage_token_index(base_token_mint_ai.key, target_leverage)
                .is_none(),
            QuasarErrorCode::Default
        )?;

        let token_index = quasar_group.num_leverage_tokens;

        // Make sure leverage token at this index not already initialized
        check!(
            quasar_group.leverage_tokens[token_index].is_empty(),
            QuasarErrorCode::Default
        )?;

        init_mango_account(
            mango_program_ai,
            mango_group_ai,
            mango_account_ai,
            admin_ai,
            &[],
        )?;

        create_mint_account(
            program_id,
            admin_ai,
            mint_ai,
            token_program_ai,
            system_program_ai,
            rent_program_ai,
            "leverage_token",
            LEVERGAE_TOKEN_DECIMALS,
        )?;

        quasar_group.leverage_tokens[token_index] = LeverageToken {
            mint: *mint_ai.key,
            base_token_mint: *base_token_mint_ai.key,
            target_leverage: target_leverage,
            mango_account: *mango_account_ai.key,
            mango_perp_market: *mango_perp_market_ai.key,
        };
        quasar_group.num_base_tokens += 1;

        Ok(())
    }

    #[inline(never)]
    fn mint_leverage_token<'a>(
        program_id: &Pubkey,
        accounts: &[AccountInfo<'a>],
        quantity: u64,
    ) -> QuasarResult {
        Ok(())
    }

    #[inline(never)]
    fn redeem_leverage_token<'a>(
        program_id: &Pubkey,
        accounts: &[AccountInfo<'a>],
        quantity: u64,
    ) -> QuasarResult {
        Ok(())
    }

    #[inline(never)]
    fn test_create_account<'a>(program_id: &Pubkey, accounts: &[AccountInfo<'a>]) -> QuasarResult {
        const NUM_FIXED: usize = 4;
        let accounts = array_ref![accounts, 0, NUM_FIXED];

        let [
            signer_ai,        // write
            owner_ai,        // read
            new_account_ai,
            system_program_ai,
        ] = accounts;

        create_account(
            &signer_ai,
            new_account_ai,
            10,
            &owner_ai,
            &system_program_ai,
        )?;

        Ok(())
    }

    #[inline(never)]
    fn test_initialize_mint<'a>(program_id: &Pubkey, accounts: &[AccountInfo<'a>]) -> QuasarResult {
        const NUM_FIXED: usize = 6;
        let accounts = array_ref![accounts, 0, NUM_FIXED];

        let [
            program_ai,
            signer_ai,
            mint_ai,        // write
            token_program_ai,
            system_program_ai,
            rent_program_ai,
        ] = accounts;

        create_mint_account(
            program_id,
            signer_ai,
            mint_ai,
            token_program_ai,
            system_program_ai,
            rent_program_ai,
            "leverage_token",
            LEVERGAE_TOKEN_DECIMALS,
        )?;

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

fn init_mango_account<'a>(
    mango_program_ai: &AccountInfo<'a>,
    mango_group_ai: &AccountInfo<'a>,
    mango_account_ai: &AccountInfo<'a>,
    signer_ai: &AccountInfo<'a>,
    signers_seeds: &[&[&[u8]]],
) -> ProgramResult {
    let instruction = Instruction {
        program_id: *mango_program_ai.key,
        data: mango::instruction::MangoInstruction::InitMangoAccount.pack(),
        accounts: vec![
            AccountMeta::new_readonly(*mango_group_ai.key, false),
            AccountMeta::new(*mango_account_ai.key, false),
            AccountMeta::new_readonly(*signer_ai.key, true),
        ],
    };

    let account_infos = [
        mango_program_ai.clone(),
        mango_group_ai.clone(),
        mango_account_ai.clone(),
        signer_ai.clone(),
    ];

    invoke_signed(&instruction, &account_infos, signers_seeds)
}

fn create_mint_account<'a>(
    program_id: &Pubkey,
    signer_ai: &AccountInfo<'a>,
    mint_ai: &AccountInfo<'a>, // write
    token_program_ai: &AccountInfo<'a>,
    system_program_ai: &AccountInfo<'a>,
    rent_program_ai: &AccountInfo<'a>,
    seed: &str,
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

    let (pda, bump_seed) = Pubkey::find_program_address(&[seed.as_bytes()], program_id);

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
        &pda,
        Some(&pda),
        decimals,
    )?;

    solana_program::program::invoke_signed(
        &instruction,
        &[
            mint_ai.clone(),
            token_program_ai.clone(),
            rent_program_ai.clone(),
        ],
        &[&[seed.as_bytes(), &[bump_seed]]],
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
