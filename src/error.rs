use thiserror::Error;

use bytemuck::Contiguous;
use solana_program::program_error::ProgramError;

use mango;
use num_enum::IntoPrimitive;

pub type QuasarResult<T = ()> = Result<T, QuasarError>;

#[repr(u8)]
#[derive(Debug, Clone, Eq, PartialEq, Copy)]
pub enum SourceFileId {
    Processor = 0,
    State = 1,
    Oracle = 2,
}

impl std::fmt::Display for SourceFileId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SourceFileId::Processor => write!(f, "src/processor.rs"),
            SourceFileId::State => write!(f, "src/state.rs"),
            SourceFileId::Oracle => write!(f, "src/oracle.rs"),
        }
    }
}

#[derive(Error, Debug, PartialEq, Eq)]
pub enum QuasarError {
    #[error(transparent)]
    ProgramError(#[from] ProgramError),
    #[error("{quasar_error_code}; {source_file_id}:{line}")]
    QuasarErrorCode {
        quasar_error_code: QuasarErrorCode,
        line: u32,
        source_file_id: SourceFileId,
    },
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq, IntoPrimitive)]
#[repr(u32)]
pub enum QuasarErrorCode {
    /// Invalid instruction
    #[error("QuasarErrorCode::InvalidInstruction")]
    InvalidInstruction,
    #[error("QuasarErrorCode::InvalidOwner")]
    InvalidOwner,
    #[error("QuasarErrorCode::InvalidGroupOwner")]
    InvalidGroupOwner,
    #[error("QuasarErrorCode::InvalidSignerKey")]
    InvalidSignerKey,
    #[error("QuasarErrorCode::InvalidAdminKey")]
    InvalidAdminKey,
    #[error("QuasarErrorCode::InsufficientFunds")]
    InsufficientFunds,
    #[error("QuasarErrorCode::InvalidToken")]
    InvalidToken,
    #[error("QuasarErrorCode::InvalidProgramId")]
    InvalidProgramId,
    #[error("QuasarErrorCode::GroupNotRentExempt")]
    GroupNotRentExempt,
    #[error("QuasarErrorCode::AccountNotRentExempt")]
    AccountNotRentExempt,
    #[error("QuasarErrorCode::OutOfSpace")]
    OutOfSpace,

    #[error("QuasarErrorCode::InvalidParam")]
    InvalidParam,
    #[error("QuasarErrorCode::InvalidAccount")]
    InvalidAccount,
    #[error("QuasarErrorCode::SignerNecessary")]
    SignerNecessary,

    #[error("QuasarErrorCode::Default Check the source code for more info")]
    Default = u32::MAX_VALUE,
}

impl From<QuasarError> for ProgramError {
    fn from(e: QuasarError) -> ProgramError {
        match e {
            QuasarError::ProgramError(pe) => pe,
            QuasarError::QuasarErrorCode {
                quasar_error_code,
                line: _,
                source_file_id: _,
            } => ProgramError::Custom(quasar_error_code.into()),
        }
    }
}

impl From<mango::error::MangoError> for QuasarError {
    fn from(me: mango::error::MangoError) -> Self {
        let pe: ProgramError = me.into();
        pe.into()
    }
}

#[inline]
pub fn check_assert(
    cond: bool,
    quasar_error_code: QuasarErrorCode,
    line: u32,
    source_file_id: SourceFileId,
) -> QuasarResult<()> {
    if cond {
        Ok(())
    } else {
        Err(QuasarError::QuasarErrorCode {
            quasar_error_code,
            line,
            source_file_id,
        })
    }
}

#[macro_export]
macro_rules! declare_check_assert_macros {
    ($source_file_id:expr) => {
        #[allow(unused_macros)]
        macro_rules! check {
            ($cond:expr, $err:expr) => {
                check_assert($cond, $err, line!(), $source_file_id)
            };
        }

        #[allow(unused_macros)]
        macro_rules! check_eq {
            ($x:expr, $y:expr, $err:expr) => {
                check_assert($x == $y, $err, line!(), $source_file_id)
            };
        }

        #[allow(unused_macros)]
        macro_rules! throw {
            () => {
                QuasarError::QuasarErrorCode {
                    quasar_error_code: QuasarErrorCode::Default,
                    line: line!(),
                    source_file_id: $source_file_id,
                }
            };
        }

        #[allow(unused_macros)]
        macro_rules! throw_err {
            ($err:expr) => {
                QuasarError::QuasarErrorCode {
                    quasar_error_code: $err,
                    line: line!(),
                    source_file_id: $source_file_id,
                }
            };
        }

        #[allow(unused_macros)]
        macro_rules! math_err {
            () => {
                QuasarError::QuasarErrorCode {
                    quasar_error_code: QuasarErrorCode::MathError,
                    line: line!(),
                    source_file_id: $source_file_id,
                }
            };
        }
    };
}
