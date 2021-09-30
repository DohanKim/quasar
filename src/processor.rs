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
    system_instruction,
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
    oracle::{determine_oracle_type, OracleType, StubOracle},
    state::{BaseToken, DataType, LeverageToken, MetaData, QuasarGroup},
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
            QuasarInstruction::Test => {
                msg!("Instruction: Test");
                Self::test(program_id, accounts)
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
            OracleType::Switchboard => {
                msg!("OracleType::Switchboard");
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

    // fn init_mango_account<'a>(program_id: &Pubkey, accounts: &[AccountInfo<'a>]) -> QuasarResult {
    //     const NUM_FIXED: usize = 4;
    //     let accounts = array_ref![accounts, 0, NUM_FIXED];

    //     let [
    //         mango_prog_ai,
    //         mango_group_ai,         // read
    //         mango_account_ai,       // write
    //         signer_ai,               // read & signer
    //     ] = accounts;

    //     let instruction = Instruction {
    //         program_id: *mango_prog_ai.key,
    //         data: mango::instruction::MangoInstruction::InitMangoAccount.pack(),
    //         accounts: vec![
    //             AccountMeta::new_readonly(*mango_group_ai.key, false),
    //             AccountMeta::new(*mango_account_ai.key, false),
    //             AccountMeta::new_readonly(*signer_ai.key, true),
    //         ],
    //     };

    //     let account_infos = [
    //         mango_prog_ai.clone(),
    //         mango_group_ai.clone(),
    //         mango_account_ai.clone(),
    //         signer_ai.clone(),
    //     ];

    //     invoke(&instruction, &account_infos);

    //     Ok(())
    // }

    #[inline(never)]
    /// Add a leveraged token to quasar group
    /// Only allow admin
    fn add_leverage_token(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        target_leverage: I80F48,
    ) -> QuasarResult {
        const NUM_FIXED: usize = 6;
        let accounts = array_ref![accounts, 0, NUM_FIXED];
        let [
            quasar_group_ai, // write
            mint_ai,        // read
            base_token_mint_ai,        // read
            mango_account_ai, // read
            mango_perp_market_ai,
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
    fn test<'a>(program_id: &Pubkey, accounts: &[AccountInfo<'a>]) -> QuasarResult {
        const NUM_FIXED: usize = 4;
        let accounts = array_ref![accounts, 0, NUM_FIXED];

        let [
            signer_ai,        // write
            owner_ai,        // read
            new_account_ai,
            system_program_ai,
        ] = accounts;
        msg!(&signer_ai.key.to_string());
        msg!(&owner_ai.key.to_string());

        // let account = Pubkey::new_unique();
        // msg!(&account.to_string());

        create_account(
            &signer_ai,
            new_account_ai,
            10,
            &owner_ai,
            &system_program_ai,
        );

        Ok(())
    }
}

// #[allow(dead_code)]
// pub async fn create_account(&mut self, size: usize, owner: &Pubkey) -> Pubkey {
//     let keypair = Keypair::new();
//     let rent = self.rent.minimum_balance(size);

//     let instructions = [system_instruction::create_account(
//         &self.context.payer.pubkey(),
//         &keypair.pubkey(),
//         rent as u64,
//         size as u64,
//         owner,
//     )];

//     self.process_transaction(&instructions, Some(&[&keypair]))
//         .await
//         .unwrap();

//     return keypair.pubkey();
// }

fn create_account<'a>(
    signer_ai: &AccountInfo<'a>,
    new_account_ai: &AccountInfo<'a>,
    space: usize,
    owner_ai: &AccountInfo<'a>,
    system_program_ai: &AccountInfo<'a>,
) -> ProgramResult {
    let rent = Rent::default().minimum_balance(space);

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
