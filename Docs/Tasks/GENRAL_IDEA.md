About Superteam Ukraine
Superteam Ukraine is focused on onboarding the next generation of developers and founders in Ukraine into the Solana ecosystem. We believe in the power of community and aim to connect talented individuals with opportunities in the Solana space. 

Mission
Create a reference implementation for a Solana Yield Adapter Standard, including a core dispatcher contract, five reference adapters, an on-chain registry, mainnet-fork tests, and comprehensive developer documentation.

Scope Detail
Core Dispatcher Contract: Develop an Anchor program to act as a router with a standardized interface: deposit, withdraw, current_value.

Five Reference Adapters: Build adapters for:

Kamino USDC (Kamino Finance)

MarginFi USDC (MarginFi)

Jupiter LP (Jupiter)

Maple Syrup (Maple Finance)

Drift Insurance Fund (Drift Protocol)

On-Chain Adapter Registry: Implement a governance-gated approval mechanism for registering new adapters.

Mainnet-Fork Tests: Develop integration tests for all five adapters, to be run against mainnet state.

Developer Specification: Write an adapter standard specification and a "Build your own adapter" guide, aiming for new teams to ship a working adapter in less than a day.

Tech stack: Anchor 0.31.1, Solana 2.2.20, Rust, TypeScript

Submission Requirements
Public GitHub repository containing all source code.

All five adapters must pass mainnet-fork tests.

The registry contract needs to be deployed to devnet.

Adapter standard specification in markdown format.

"How to build your own adapter" developer guide.

Judging Criteria
Correctness: All adapters function correctly against the mainnet-fork (40%).

Interface Design: The standard is clean, minimal, and extensible (25%).

Developer Guide Quality: A new team can easily follow it and build an adapter in a day (20%).

Code Quality and Test Coverage: Overall quality of the code and thoroughness of tests (15%).

