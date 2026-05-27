## The **brewing-rep-oracle** program

[![Build Status](https://github.com/gear-tech/brewing-rep-oracle/workflows/CI/badge.svg)](https://github.com/gear-tech/brewing-rep-oracle/actions)

Program **brewing-rep-oracle** for [⚙️ Gear Protocol](https://github.com/gear-tech/gear) written in [⛵ Sails](https://github.com/gear-tech/sails) framework.

The program workspace includes the following packages:
- `brewing-rep-oracle` is the package allowing to build WASM binary for the program and IDL file for it.
  The package also includes integration tests for the program in the `tests` sub-folder
- `brewing-rep-oracle-app` is the package containing business logic for the program represented by the `BrewingRepOracle` structure.
- `brewing-rep-oracle-client` is the package containing the client for the program allowing to interact with it from another program, tests, or off-chain client.

### 🏗️ Building

```bash
cargo build --release
```

### ✅ Testing

```bash
cargo test --release
```

> For off-chain integration tests against a running node, add the `gclient` feature:
>
> ```bash
> cargo add sails-rs --dev --features gclient
> ```

# License

The source code is licensed under the [MIT license](LICENSE).