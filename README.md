# Transactions

This repository contains my implementation of the transactions engine coding test.

## Overview

The input csv is streamed line-by-line to minimize memory usage. Transaction types are represented as enums for the same reason.

The transaction processing logic is separated from the `main` entry point to facilitate unit testing. I used ChatGPT 4o to help generate a suite of unit tests which covers expected behavior and edge cases.

I chose to use the `rust_decimal` package to represent currencies. When working with currencies in the past, it has been important to represent them exactly without having to deal with floating point inaccuracies.

Finally, I interpreted the spec to mean that all output values should be formatted to 4 decimal places.

## Caveats

One point that is unclear in the spec is how to handle a dispute following a full withdrawal of funds. For example:

1. Deposit 5.0
2. Withdraw 5.0
3. Dispute the original deposit

In this case, my implementation _ignores the dispute_ since the funds are no longer available to be held. This models behavior consistent with real-world banking systems. If the automated testing expects `available` to go negative in this case, then this is why it doesn't.

## Usage

To run the code on a sample csv:

```
cargo run -- test.csv
```

To run the unit tests:

```
cargo test
```
