use anyhow::Result;
use clap::Parser;
use csv::{Reader, ReaderBuilder};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::fs::File;

/// The command line arguments.
#[derive(Parser, Default)]
struct Args {
    /// The filename to process.
    pub filename: String
}

/// The type of transaction.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
enum TransactionType {
    Deposit,
    Withdrawal,
    Dispute,
    Resolve,
    Chargeback
}

/// Represents a transaction.
#[derive(Deserialize, Debug)]
struct Transaction {
    /// The transaction type.
    #[serde(rename = "type")]
    pub tx_type: TransactionType,

    /// The client id.
    pub client: u16,

    /// The transaction id.
    pub tx: u32,

    /// The amount.
    pub amount: Option<Decimal>
}

/// The entry point.
fn main() -> Result<()> {
    let args = Args::parse();
    let file = File::open(&args.filename)?;

    // The reader needs to allow for missing columns.
    let mut reader = ReaderBuilder::new()
        .flexible(true)
        .from_reader(file);

    // Read line by line to minimize our memory footprint.
    for tx in reader.deserialize() {
        let tx: Transaction = tx?;

        println!("{:?}", tx);
        if let Some(amount) = &tx.amount {
            println!("amount: {}", amount)
        }
    }

    Ok(())
}
