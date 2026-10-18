use super::{Info, error};
use crate::{ErrorKind, Result};

/// Host-only dialogue choice, separate from C_INFO's registered fields.
#[derive(Eq, PartialEq)]
pub struct InfoChoice {
    pub text: Vec<u8>,
    /// Raw script function-symbol index, preserving the signed wire word.
    pub function: i32,
}
impl std::fmt::Debug for InfoChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InfoChoice")
            .field("text_bytes", &self.text.len())
            .field("function", &self.function)
            .finish()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ChoiceLimits {
    pub choices: usize,
    pub text_bytes: usize,
    pub total_text_bytes: usize,
}
impl Default for ChoiceLimits {
    fn default() -> Self {
        Self {
            choices: 4096,
            text_bytes: 1024 * 1024,
            total_text_bytes: 8 * 1024 * 1024,
        }
    }
}
impl Info {
    pub fn choices(&self) -> &[InfoChoice] {
        &self.choices
    }
    /// Insert at the front. Failure preserves choices.
    pub fn add_choice(&mut self, text: &[u8], function: i32, limits: ChoiceLimits) -> Result<()> {
        if self.choices.len() >= limits.choices || text.len() > limits.text_bytes {
            return Err(error(ErrorKind::LimitExceeded));
        }
        let total = self
            .choices
            .iter()
            .try_fold(text.len(), |sum, c| sum.checked_add(c.text.len()));
        if total.is_none_or(|n| n > limits.total_text_bytes) {
            return Err(error(ErrorKind::LimitExceeded));
        }
        let mut owned = Vec::new();
        owned
            .try_reserve_exact(text.len())
            .map_err(|_| error(ErrorKind::LimitExceeded))?;
        owned.extend_from_slice(text);
        self.choices
            .try_reserve(1)
            .map_err(|_| error(ErrorKind::LimitExceeded))?;
        self.choices.insert(
            0,
            InfoChoice {
                text: owned,
                function,
            },
        );
        Ok(())
    }
    pub fn remove_choice(&mut self, index: usize) -> Result<InfoChoice> {
        if index >= self.choices.len() {
            return Err(error(ErrorKind::InvalidData));
        }
        Ok(self.choices.remove(index))
    }
}
