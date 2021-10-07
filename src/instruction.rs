use arrayref::{array_ref, array_refs};
use fixed::types::I80F48;
use solana_program::program_error::ProgramError;
use std::convert::TryInto;

pub enum QuasarInstruction {
    /// Initialize a quasar group account
    ///
    /// Accounts expected by this instruction (12):
    ///
    /// 0. `[writable]` quasar_group_ai
    /// 1. `[signer]` signer_ai
    /// 2. `[]` admin_ai
    /// 3. `[]` mango_program_ai    
    InitQuasarGroup {
        signer_nonce: u64,
    },

    /// Add a base token which leveraged tokens are going to use as the underlying
    ///
    /// Accounts expected by this instruction (8):
    ///
    /// 0. `[writable]` quasar_group_ai
    /// 1. `[]` mint_ai
    /// 2. `[]` oracle_ai
    /// 3. `[signer]` admin_ai
    AddBaseToken,

    /// Add a leveraged token
    ///
    /// Accounts expected by this instruction (8):
    ///
    /// 0. `[writable]` quasar_group_ai
    /// 1. `[]` mint_ai
    /// 2. `[]` base_token_mint_ai
    /// 3. `[]` mango_account_ai
    /// 4. `[]` mango_perp_market_ai
    /// 5. `[signer]` admin_ai
    AddLeverageToken {
        target_leverage: I80F48,
    },

    /// mint a leveraged token
    ///
    /// Accounts expected by this instruction (8):
    ///
    /// 0. `[writable]` quasar_group_ai
    /// 2. `[]` leverage_token_ai
    /// 3. `[]` mango_account_ai
    /// 4. `[]` mint_ai
    /// 4. `[]` base_token_mint_ai
    /// 4. `[]` oracle_ai
    /// 8. `[signer]` admin_ai
    MintLeverageToken {
        quantity: u64,
    },

    /// redeem a leveraged token
    ///
    /// Accounts expected by this instruction (8):
    ///
    /// 0. `[writable]` quasar_group_ai
    /// 2. `[]` leverage_token_ai
    /// 3. `[]` mango_program_ai
    /// 4. `[]` mint_ai
    /// 4. `[]` base_token_mint_ai
    /// 4. `[]` oracle_ai
    /// 8. `[signer]` admin_ai
    RedeemLeverageToken {
        quantity: u64,
    },

    // only for test purpose
    TestCreateAccount,

    // only for test purpose
    TestInitializeMint,
}

impl QuasarInstruction {
    pub fn unpack(input: &[u8]) -> Option<Self> {
        let (&discrim, data) = array_refs![input, 4; ..;];
        let discrim = u32::from_le_bytes(discrim);

        Some(match discrim {
            0 => {
                let signer_nonce = array_ref![data, 0, 8];

                Self::InitQuasarGroup {
                    signer_nonce: u64::from_le_bytes(*signer_nonce),
                }
            }
            1 => Self::AddBaseToken,
            2 => {
                let target_leverage = array_ref![data, 0, 16];
                QuasarInstruction::AddLeverageToken {
                    target_leverage: I80F48::from_le_bytes(*target_leverage),
                }
            }
            3 => {
                let quantity = array_ref![data, 0, 8];
                QuasarInstruction::MintLeverageToken {
                    quantity: u64::from_le_bytes(*quantity),
                }
            }
            4 => {
                let quantity = array_ref![data, 0, 8];
                QuasarInstruction::RedeemLeverageToken {
                    quantity: u64::from_le_bytes(*quantity),
                }
            }
            5 => QuasarInstruction::TestCreateAccount,
            6 => QuasarInstruction::TestInitializeMint,
            _ => return None,
        })
    }

    fn unpack_i80f48_opt(data: &[u8; 17]) -> Option<I80F48> {
        let (opt, val) = array_refs![data, 1, 16];
        if opt[0] == 0 {
            None
        } else {
            Some(I80F48::from_le_bytes(*val))
        }
    }
    fn unpack_u64_opt(data: &[u8; 9]) -> Option<u64> {
        let (opt, val) = array_refs![data, 1, 8];
        if opt[0] == 0 {
            None
        } else {
            Some(u64::from_le_bytes(*val))
        }
    }
}
