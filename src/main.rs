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

                // Only allow the dispute if we have available funds.
                // This was unclear in the spec, but it aligns with
                // what I'd expect from a bank.
                if client.available < value.amount.unwrap() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn deposit_increases_available_and_total() {
        let txs = vec![Ok(Transaction {
            kind:   TransactionType::Deposit,
            client: 1,
            tx:     1,
            amount: Some(dec!(10.0))
        })];

        let clients = process(txs).unwrap();
        let client = clients.get(&1).unwrap();

        assert_eq!(client.available, dec!(10.0));
        assert_eq!(client.total, dec!(10.0));
        assert_eq!(client.held, dec!(0.0));
        assert!(!client.locked);
    }

    #[test]
    fn withdrawal_reduces_available_and_total() {
        let txs = vec![
            Ok(Transaction {
                kind:   TransactionType::Deposit,
                client: 1,
                tx:     1,
                amount: Some(dec!(5.0))
            }),
            Ok(Transaction {
                kind:   TransactionType::Withdrawal,
                client: 1,
                tx:     2,
                amount: Some(dec!(3.0))
            }),
        ];

        let clients = process(txs).unwrap();
        let client = clients.get(&1).unwrap();

        assert_eq!(client.available, dec!(2.0));
        assert_eq!(client.total, dec!(2.0));
    }

    #[test]
    fn withdrawal_fails_if_insufficient_funds() {
        let txs = vec![Ok(Transaction {
            kind:   TransactionType::Withdrawal,
            client: 1,
            tx:     1,
            amount: Some(dec!(10.0))
        })];

        let clients = process(txs).unwrap();
        let client = clients.get(&1).unwrap();

        assert_eq!(client.available, dec!(0.0));
        assert_eq!(client.total, dec!(0.0));
    }

    #[test]
    fn dispute_moves_funds_to_held() {
        let txs = vec![
            Ok(Transaction {
                kind:   TransactionType::Deposit,
                client: 1,
                tx:     1,
                amount: Some(dec!(5.0))
            }),
            Ok(Transaction {
                kind:   TransactionType::Dispute,
                client: 1,
                tx:     1,
                amount: None
            }),
        ];

        let clients = process(txs).unwrap();
        let client = clients.get(&1).unwrap();

        assert_eq!(client.available, dec!(0.0));
        assert_eq!(client.held, dec!(5.0));
        assert_eq!(client.total, dec!(5.0));
        assert!(!client.locked);
    }

    #[test]
    fn dispute_twice_does_nothing_the_second_time() {
        let txs = vec![
            Ok(Transaction {
                kind:   TransactionType::Deposit,
                client: 1,
                tx:     1,
                amount: Some(dec!(5.0))
            }),
            Ok(Transaction {
                kind:   TransactionType::Dispute,
                client: 1,
                tx:     1,
                amount: None
            }),
            Ok(Transaction {
                kind:   TransactionType::Dispute,
                client: 1,
                tx:     1,
                amount: None
            }),
        ];

        let clients = process(txs).unwrap();
        let client = clients.get(&1).unwrap();

        assert_eq!(client.available, dec!(0.0));
        assert_eq!(client.held, dec!(5.0));
        assert_eq!(client.total, dec!(5.0));
    }

    #[test]
    fn dispute_is_ignored_if_funds_already_withdrawn() {
        let txs = vec![
            Ok(Transaction {
                kind:   TransactionType::Deposit,
                client: 1,
                tx:     1,
                amount: Some(dec!(5.0))
            }),
            Ok(Transaction {
                kind:   TransactionType::Withdrawal,
                client: 1,
                tx:     2,
                amount: Some(dec!(5.0))
            }),
            Ok(Transaction {
                kind:   TransactionType::Dispute,
                client: 1,
                tx:     1,
                amount: None
            }),
        ];

        let clients = process(txs).unwrap();
        let client = clients.get(&1).unwrap();

        assert_eq!(client.available, dec!(0.0));
        assert_eq!(client.held, dec!(0.0));
        assert_eq!(client.total, dec!(0.0));
        assert!(!client.locked);
    }

    #[test]
    fn resolve_returns_held_to_available() {
        let txs = vec![
            Ok(Transaction {
                kind:   TransactionType::Deposit,
                client: 1,
                tx:     1,
                amount: Some(dec!(7.5))
            }),
            Ok(Transaction {
                kind:   TransactionType::Dispute,
                client: 1,
                tx:     1,
                amount: None
            }),
            Ok(Transaction {
                kind:   TransactionType::Resolve,
                client: 1,
                tx:     1,
                amount: None
            }),
        ];

        let clients = process(txs).unwrap();
        let client = clients.get(&1).unwrap();

        assert_eq!(client.available, dec!(7.5));
        assert_eq!(client.held, dec!(0.0));
        assert_eq!(client.total, dec!(7.5));
        assert!(!client.locked);
    }

    #[test]
    fn resolve_ignored_if_tx_not_disputed() {
        let txs = vec![
            Ok(Transaction {
                kind:   TransactionType::Deposit,
                client: 1,
                tx:     1,
                amount: Some(dec!(5.0))
            }),
            Ok(Transaction {
                kind:   TransactionType::Resolve,
                client: 1,
                tx:     1,
                amount: None
            }),
        ];

        let clients = process(txs).unwrap();
        let client = clients.get(&1).unwrap();

        assert_eq!(client.available, dec!(5.0));
        assert_eq!(client.held, dec!(0.0));
        assert_eq!(client.total, dec!(5.0));
        assert!(!client.locked);
    }

    #[test]
    fn chargeback_removes_held_and_locks() {
        let txs = vec![
            Ok(Transaction {
                kind:   TransactionType::Deposit,
                client: 1,
                tx:     1,
                amount: Some(dec!(3.0))
            }),
            Ok(Transaction {
                kind:   TransactionType::Dispute,
                client: 1,
                tx:     1,
                amount: None
            }),
            Ok(Transaction {
                kind:   TransactionType::Chargeback,
                client: 1,
                tx:     1,
                amount: None
            }),
        ];

        let clients = process(txs).unwrap();
        let client = clients.get(&1).unwrap();

        assert_eq!(client.available, dec!(0.0));
        assert_eq!(client.held, dec!(0.0));
        assert_eq!(client.total, dec!(0.0));
        assert!(client.locked);
    }

    #[test]
    fn chargeback_ignored_if_tx_not_disputed() {
        let txs = vec![
            Ok(Transaction {
                kind:   TransactionType::Deposit,
                client: 1,
                tx:     1,
                amount: Some(dec!(5.0))
            }),
            Ok(Transaction {
                kind:   TransactionType::Chargeback,
                client: 1,
                tx:     1,
                amount: None
            }),
        ];

        let clients = process(txs).unwrap();
        let client = clients.get(&1).unwrap();

        assert_eq!(client.available, dec!(5.0));
        assert_eq!(client.held, dec!(0.0));
        assert_eq!(client.total, dec!(5.0));
        assert!(!client.locked);
    }

    #[test]
    fn locked_account_ignores_future_transactions() {
        let txs = vec![
            Ok(Transaction {
                kind:   TransactionType::Deposit,
                client: 1,
                tx:     1,
                amount: Some(dec!(10.0))
            }),
            Ok(Transaction {
                kind:   TransactionType::Dispute,
                client: 1,
                tx:     1,
                amount: None
            }),
            Ok(Transaction {
                kind:   TransactionType::Chargeback,
                client: 1,
                tx:     1,
                amount: None
            }),
            Ok(Transaction {
                kind:   TransactionType::Deposit,
                client: 1,
                tx:     2,
                amount: Some(dec!(5.0))
            }),
        ];

        let clients = process(txs).unwrap();
        let client = clients.get(&1).unwrap();

        assert_eq!(client.available, dec!(0.0));
        assert_eq!(client.total, dec!(0.0));
        assert!(client.locked);
    }

    #[test]
    fn handles_multiple_clients_independently() {
        let txs = vec![
            Ok(Transaction {
                kind:   TransactionType::Deposit,
                client: 1,
                tx:     1,
                amount: Some(dec!(10.0))
            }),
            Ok(Transaction {
                kind:   TransactionType::Withdrawal,
                client: 1,
                tx:     2,
                amount: Some(dec!(4.0))
            }),
            Ok(Transaction {
                kind:   TransactionType::Deposit,
                client: 2,
                tx:     3,
                amount: Some(dec!(20.0))
            }),
            Ok(Transaction {
                kind:   TransactionType::Withdrawal,
                client: 2,
                tx:     4,
                amount: Some(dec!(15.0))
            }),
        ];

        let clients = process(txs).unwrap();

        let c1 = clients.get(&1).unwrap();
        assert_eq!(c1.available, dec!(6.0));
        assert_eq!(c1.total, dec!(6.0));
        assert_eq!(c1.held, dec!(0.0));
        assert!(!c1.locked);

        let c2 = clients.get(&2).unwrap();
        assert_eq!(c2.available, dec!(5.0));
        assert_eq!(c2.total, dec!(5.0));
        assert_eq!(c2.held, dec!(0.0));
        assert!(!c2.locked);
    }
}
