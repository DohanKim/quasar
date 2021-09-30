use fixed::types::I80F48;
use mango_common::Loadable;
use mango_macro::{Loadable, Pod};
use num_enum::{IntoPrimitive, TryFromPrimitive};

use solana_program::{
    account_info::AccountInfo,
    program_error::ProgramError,
    program_pack::{IsInitialized, Pack, Sealed},
    pubkey::Pubkey,
};

use arrayref::{array_mut_ref, array_ref, array_refs, mut_array_refs};

use std::cell::{Ref, RefMut};

use crate::error::{check_assert, QuasarError, QuasarErrorCode, QuasarResult, SourceFileId};

declare_check_assert_macros!(SourceFileId::State);

pub const MAX_BASE_TOKENS: usize = 16;
pub const MAX_LEVERAGE_TOKENS: usize = 32;

#[repr(u8)]
#[derive(IntoPrimitive, TryFromPrimitive)]
pub enum DataType {
    QuasarGroup = 0,
}

#[derive(Copy, Clone, Pod, Default)]
#[repr(C)]
/// Stores meta information about the `Account` on chain
pub struct MetaData {
    pub data_type: u8,
    pub version: u8,
    pub is_initialized: bool,
    pub padding: [u8; 5], // This makes explicit the 8 byte alignment padding
}

impl MetaData {
    pub fn new(data_type: DataType, version: u8, is_initialized: bool) -> Self {
        Self {
            data_type: data_type as u8,
            version,
            is_initialized,
            padding: [0u8; 5],
        }
    }
}

#[derive(Copy, Clone, Pod, Loadable)]
#[repr(C)]
pub struct QuasarGroup {
    pub meta_data: MetaData,

    pub num_base_tokens: usize,
    pub base_tokens: [BaseToken; MAX_BASE_TOKENS],

    pub num_leverage_tokens: usize,
    pub leverage_tokens: [LeverageToken; MAX_LEVERAGE_TOKENS],

    pub signer_key: Pubkey,
    pub admin_key: Pubkey,
    pub mango_program_id: Pubkey,
}

impl QuasarGroup {
    pub fn load_mut_checked<'a>(
        account: &'a AccountInfo,
        program_id: &Pubkey,
    ) -> QuasarResult<RefMut<'a, Self>> {
        check_eq!(account.owner, program_id, QuasarErrorCode::InvalidOwner)?;

        let quasar_group: RefMut<'a, Self> = Self::load_mut(account)?;
        check!(
            quasar_group.meta_data.is_initialized,
            QuasarErrorCode::InvalidAccount
        )?;
        check_eq!(
            quasar_group.meta_data.data_type,
            DataType::QuasarGroup as u8,
            QuasarErrorCode::InvalidAccount
        )?;

        Ok(quasar_group)
    }

    pub fn load_checked<'a>(
        account: &'a AccountInfo,
        program_id: &Pubkey,
    ) -> QuasarResult<Ref<'a, Self>> {
        check_eq!(account.owner, program_id, QuasarErrorCode::InvalidOwner)?;

        let quasar_group: Ref<'a, Self> = Self::load(account)?;
        check!(
            quasar_group.meta_data.is_initialized,
            QuasarErrorCode::InvalidAccount
        )?;
        check_eq!(
            quasar_group.meta_data.data_type,
            DataType::QuasarGroup as u8,
            QuasarErrorCode::InvalidAccount
        )?;

        Ok(quasar_group)
    }

    pub fn find_leverage_token_index(
        &self,
        base_token_mint: &Pubkey,
        target_leverage: I80F48,
    ) -> Option<usize> {
        self.leverage_tokens.iter().position(|lt| {
            lt.base_token_mint == *base_token_mint && lt.target_leverage == target_leverage
        })
    }

    pub fn find_base_token_index(&self, base_token_mint: &Pubkey) -> Option<usize> {
        self.base_tokens
            .iter()
            .position(|bt| bt.mint == *base_token_mint)
    }
}

#[derive(Copy, Clone, Pod)]
#[repr(C)]
pub struct BaseToken {
    pub mint: Pubkey,
    pub decimals: u8,
    pub oracle: Pubkey,
    pub padding: [u8; 7],
}

impl BaseToken {
    pub fn is_empty(&self) -> bool {
        self.mint == Pubkey::default()
    }
}

#[derive(Copy, Clone, Pod)]
#[repr(C)]
pub struct LeverageToken {
    pub mint: Pubkey,
    pub base_token_mint: Pubkey,
    pub target_leverage: I80F48,
    pub mango_account: Pubkey,
    pub mango_perp_market: Pubkey,
}

impl LeverageToken {
    pub fn is_empty(&self) -> bool {
        self.mint == Pubkey::default()
    }
}
