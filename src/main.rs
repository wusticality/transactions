use anyhow::{anyhow, Result};
use clap::Parser;
use csv::ReaderBuilder;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    fs::File
};

/// The command line arguments.
#[derive(Parser, Default)]
struct Args {
    /// The filename to process.
    pub filename: String
}

/// The transaction type.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
enum TransactionType {
    Deposit,
    Withdrawal,
    Dispute,
    Resolve,
    Chargeback
}

/// A transaction.
#[derive(Deserialize, Debug)]
struct Transaction {
    /// The transaction type.
    #[serde(rename = "type")]
    pub kind: TransactionType,

    /// The client id.
    pub client: u16,

    /// The transaction id.
    pub tx: u32,

    /// The amount.
    pub amount: Option<Decimal>
}

impl Transaction {
    /// Makes sure the transactions are well-formed.
    fn verify(&self) -> Result<()> {
        match self.kind {
            TransactionType::Deposit | TransactionType::Withdrawal => {
                if self.amount.is_none() {
                    return Err(anyhow!("transaction {} has no amount", self.tx));
                }
            },

            _ => {}
        }

        Ok(())
    }
}

/// Aggregated client data.
#[derive(Default, Debug)]
struct ClientData {
    pub available: Decimal,
    pub held:      Decimal,
    pub total:     Decimal,
    pub locked:    bool
}

/// The entry point.
fn main() -> Result<()> {
    let args = Args::parse();
    let file = File::open(&args.filename)?;

    // Allow for whitespace and missing columns.
    let mut reader = ReaderBuilder::new()
        .trim(csv::Trim::All)
        .flexible(true)
        .from_reader(file);
    let txs = reader
        .deserialize::<Transaction>()
        .map(|r| r.map_err(Into::into));

    // Process the transactions.
    let clients = process(txs)?;

    // Print the client data to stdout..
    println!("client,available,held,total,locked");

    for (id, client) in &clients {
        println!(
            "{},{:.4},{:.4},{:.4},{}",
            id, client.available, client.held, client.total, client.locked
        );
    }

    Ok(())
}

/// Processes transactions.
fn process<T>(txs: T) -> Result<HashMap<u16, ClientData>>
where
    T: IntoIterator<Item = Result<Transaction>>
{
    let mut clients = HashMap::<u16, ClientData>::new();
    let mut deposits = HashMap::<u32, Transaction>::new();
    let mut disputed = HashSet::<u32>::new();

    // Read line by line to minimize our memory footprint.
    for tx in txs {
        let tx: Transaction = tx?;

        // Verify the transaction.
        tx.verify()?;

        // Ensure this client exists.
        let client = clients
            .entry(tx.client)
            .or_default();

        // If the client is locked, do nothing.
        if client.locked {
            continue;
        }

        // Now match on the transaction type.
        match tx.kind {
            TransactionType::Deposit => {
                let amount = &tx.amount.unwrap();

                // Update the client data.
                client.available += amount;
                client.total += amount;

                // Store the deposit.
                deposits.insert(tx.tx, tx);
            },

            TransactionType::Withdrawal => {
                let amount = &tx.amount.unwrap();

                // Check if we have enough available funds.
                if client.available - amount < Decimal::ZERO {
                    continue;
                }

                // Update the client data.
                client.available -= amount;
                client.total -= amount;
            },

            TransactionType::Dispute => {
                // Try and lookup the disputed transaction.
                let Some(value) = deposits.get(&tx.tx) else {
                    continue;
                };

                // Make sure it's not already being disputed.
                if disputed.contains(&tx.tx) {
                    continue;
                }

                // Update the client data.
                client.available -= value.amount.unwrap();
                client.held += value.amount.unwrap();

                // Mark the transaction as disputed.
                disputed.insert(tx.tx);
            },

            TransactionType::Resolve => {
                // Try and lookup the disputed transaction.
                let Some(value) = deposits.get(&tx.tx) else {
                    continue;
                };

                // Make sure that it is being disputed.
                if !disputed.contains(&tx.tx) {
                    continue;
                }

                // Update the client data.
                client.available += value.amount.unwrap();
                client.held -= value.amount.unwrap();

                // Mark the transaction as no longer disputed.
                disputed.remove(&tx.tx);
            },

            TransactionType::Chargeback => {
                // Try and lookup the disputed transaction.
                let Some(value) = deposits.get(&tx.tx) else {
                    continue;
                };

                // Make sure that it is being disputed.
                if !disputed.contains(&tx.tx) {
                    continue;
                }

                // Update the client data.
                client.held -= value.amount.unwrap();
                client.total -= value.amount.unwrap();
                client.locked = true;

                // Mark the transaction as no longer disputed.
                disputed.remove(&tx.tx);
            }
        }
    }

    Ok(clients)
}
