use derive_builder::Builder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Builder, Serialize, Deserialize)]
pub struct Transaction {
    #[builder(default)]
    pub account_id: i64,
    pub date: chrono::NaiveDate,
    #[builder(default)]
    pub code: String,
    pub description: String,
    pub ref1: String,
    #[builder(default)]
    pub ref2: String,
    #[builder(default)]
    pub ref3: String,
    #[builder(default)]
    pub status: String,
    pub debit: Option<i64>,
    pub credit: Option<i64>,
    #[builder(setter(skip), default = "self.compute_hash()")]
    pub hash: String,
}

impl TransactionBuilder {
    fn compute_hash(&self) -> String {
        let mut h = Sha256::new();
        let payload = format!(
            "{}|{}|{}|{}|{}|{}|{}|{}",
            self.account_id.unwrap_or_default(),
            self.date
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_default(),
            self.code.as_deref().unwrap_or_default(),
            self.ref1.as_deref().unwrap_or_default(),
            self.ref2.as_deref().unwrap_or_default(),
            self.ref3.as_deref().unwrap_or_default(),
            self.debit
                .flatten()
                .map(|v| v.to_string())
                .unwrap_or_default(),
            self.credit
                .flatten()
                .map(|v| v.to_string())
                .unwrap_or_default(),
        );
        h.update(payload);
        hex::encode(h.finalize())
    }

    pub fn amount(&mut self, n: i64) -> &mut Self {
        if n < 0 {
            self.debit(Some(n.abs()));
            self.credit(None);
        } else {
            self.credit(Some(n));
            self.debit(None);
        }
        self
    }
}
