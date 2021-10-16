use fixed::types::I80F48;
use mango::state::{MangoAccount, RootBankCache, ZERO_I80F48};
use solana_program::program_error::ProgramError;
use solana_program::pubkey::Pubkey;

use bytemuck::{bytes_of, cast_slice_mut, from_bytes_mut, Contiguous, Pod};

use crate::error::QuasarResult;

pub fn gen_signer_seeds<'a>(nonce: &'a u64, acc_pk: &'a Pubkey) -> [&'a [u8]; 2] {
    [acc_pk.as_ref(), bytes_of(nonce)]
}

pub fn gen_signer_key(
    nonce: u64,
    acc_pk: &Pubkey,
    program_id: &Pubkey,
) -> Result<Pubkey, ProgramError> {
    let seeds = gen_signer_seeds(&nonce, acc_pk);
    Ok(Pubkey::create_program_address(&seeds, program_id)?)
}

pub fn get_mango_spot_value(
    mango_account: &MangoAccount,
    bank_cache: &RootBankCache,
    price: I80F48,
    market_index: usize,
) -> QuasarResult<I80F48> {
    let base_net = if mango_account.deposits[market_index].is_positive() {
        mango_account.deposits[market_index]
            .checked_mul(bank_cache.deposit_index)
            .unwrap()
    } else if mango_account.borrows[market_index].is_positive() {
        -mango_account.borrows[market_index]
            .checked_mul(bank_cache.borrow_index)
            .unwrap()
    } else {
        ZERO_I80F48
    };

    Ok(base_net * price)
}
