# SYSBREAK Smart Contracts

Smart contracts for the SYSBREAK cyberpunk hacking MMO on Shido Network (Cosmos SDK).

Production contracts are written in **CosmWasm (Rust)** targeting `wasm32-unknown-unknown`. The original Solidity/EVM prototypes are preserved under their respective directories for reference.

## Contracts

### 1. sysbreak-item-nft

CW-721 NFT contract for in-game items.

- Batch minting by authorized minter
- Custom metadata with item type, rarity, stats, and image URI
- EIP-2981-style royalty support (basis points)
- Pause/unpause by owner
- Two-step minter transfer (propose + accept)
- Approval and operator system (CW-721 compatible)

### 2. sysbreak-achievement-nft

CW-721 NFT contract for player achievements.

- Optional soulbound (non-transferable) tokens enforced across all transfer paths
- Per-player deduplication by achievement type (atomic check-and-mint)
- Achievement metadata with type, description, rarity, and earned timestamp
- Batch minting with duplicate detection
- Soulbound enforcement on TransferNft, SendNft, and Approve

### 3. sysbreak-credit-bridge

$SHIDO to in-game credits bridge with signature-verified withdrawals.

- Deposit native tokens to receive credits (tracked on-chain)
- Withdraw credits back to native tokens via secp256k1 oracle signature
- Rolling 24-hour rate limits (per-player and global)
- Nonce replay protection
- Configurable fee (basis points) and minimum withdrawal
- Peak balance tracking with reserve percentage
- Two-step oracle key rotation (propose + accept)

### 4. sysbreak-corporation-dao

Guild governance DAO with proposals, voting, and treasury management.

- Corporation lifecycle: Active, Dissolving, Dissolved
- Open and invite-only join policies
- 6 proposal types: TreasurySpend, ChangeSettings, KickMember, PromoteMember, Dissolution, Custom
- Flash-join voting protection (members must join before proposal creation to vote)
- Proposal deposit (refunded on pass, burned on fail)
- Treasury spend capped at 25% per proposal
- Dissolution requires 75% supermajority with per-member claim pattern
- Check-effects-interactions: state mutation before BankMsg dispatch

## Project Structure

```
contracts/
  Cargo.toml                              # Workspace root
  scripts/build.sh                        # Build + optimize script

  sysbreak-item-nft/                      # CosmWasm contract
  sysbreak-achievement-nft/               # CosmWasm contract
  sysbreak-credit-bridge/                 # CosmWasm contract
  sysbreak-corporation-dao/               # CosmWasm contract

  solidity-reference/                      # Original EVM prototypes
    sysbreakItemNFT/
    sysbreakAchievementNFT_contract/
    sysbreakCreditBridge/
    corporationDAO/
```

## Prerequisites

```bash
rustup target add wasm32-unknown-unknown
```

## Build

```bash
# Build all contracts for wasm32
cargo build --release --target wasm32-unknown-unknown

# Or use the build script (includes wasm-opt if available)
./scripts/build.sh
```

## Test

```bash
# Run all tests
cargo test

# Run tests for a single contract
cargo test -p sysbreak-item-nft
cargo test -p sysbreak-achievement-nft
cargo test -p sysbreak-credit-bridge
cargo test -p sysbreak-corporation-dao
```

**Test coverage:** 102 integration tests across all 4 contracts.

## Toolchain

| Dependency | Version |
|---|---|
| cosmwasm-std | 2.2 |
| cw-storage-plus | 2.0 |
| cw2 | 2.0 |
| cw721 | 0.21 |
| Rust edition | 2021 |
| Solidity (reference) | 0.8.24 |

## License

MIT
