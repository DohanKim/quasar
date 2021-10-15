## Inspiration

Centralized exchanges with perpetual futures contract markets such as FTX and Binance are using their markets to issue leveraged tokens, and more and more users are using them. We thought this was one of the missing DeFi blocks in the Solana ecosystem, and since MangoMarkets has already implemented decentralized perpetual futures contracts, we thought we could implement it quickly.

## What it does

### Quasar Leveraged Tokens

Quasar Leveraged Tokens(QLTs) are normal SPL tokens that can give you leveraged exposure to cryptocurrency markets, without the inconvenience and worry of managing a leveraged position.

Unlike existing leveraged tokens managed by centralized exchanges such as FTX and Binance, QLTs are issued in a fully decentralized manner, using MangoMarkets, which implements a decentralized perpetual futures contract.

The leveraged token will automatically reinvest or sell its position for target leverage in Mango perpetual market. If your position makes money, it will reinvest your position, or if your position loses money, it will sell your position. This task will run periodically.

### Mint QLT

When users deposit their collateral assets and create new QLTs, the Quasar program automatically deposits the collateral assets in the MangoMarkets and creates a new perpetual futures contract to match the target leverage.

### Redeem QLT

Conversely, when users redeem QLT tokens, the Quasar program sells perpetual futures contracts held in MangoMarkets and withdraws collateral assets as much as the users redeem.

### Rebalance QLT

When the price of the underlying asset of QLTs changes, the leverage also changes.
To adjust this, the Rebalance Program is executed in QuasarProtocol at regular intervals or whenever a specific leverage value is exceeded to set the target leverage.

### Environment Setup
1. Install Rust from https://rustup.rs/
2. Install Solana v1.6.2 or later from https://docs.solana.com/cli/install-solana-cli-tools#use-solanas-install-tool

### Build and test for program compiled natively
```
$ cargo build
$ cargo test
```

### Build and test the program compiled for BPF
```
$ cargo build-bpf
$ cargo test-bpf
```

### How to use
TBU
