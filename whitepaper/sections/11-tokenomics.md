# Section 11 — Tokenomics

> Tokenomics is not marketing.  
> It is the economic architecture of the protocol —  
> the rules that determine who holds value, who earns it, how it is created, and how it is destroyed.  
> A protocol with flawed tokenomics will fail regardless of its technical quality.  
> This section defines RupeeCoin's monetary policy with precision and justifies every parameter.

---

## 11.1 Design Objectives

RupeeCoin's tokenomics are designed to satisfy five objectives simultaneously:

**1. Sound monetary policy.**  
The total supply must be bounded and predictable.  
Inflation must be defined by protocol rules, not governance discretion.  
Users must be able to reason about the long-term supply of RPC.

**2. Fair initial distribution.**  
No pre-mine. No founder allocation. No venture capital allocation.  
The supply enters circulation through validator rewards and the genesis distribution process.  
Early participants are rewarded for contribution, not for proximity to founders.

**3. Permanent validator incentive.**  
Bitcoin's incentive model collapses as block rewards reach zero.  
RupeeCoin's model must sustain validator participation indefinitely through a mechanism that does not require perpetual inflation.

**4. Alignment of incentives.**  
Token holders, validators, developers, and users must have aligned long-term interests.  
A token that validators are incentivized to dump does not support a healthy network.

**5. Resistance to concentration.**  
The distribution mechanism should resist concentration of supply among a small number of actors.  
Wealth concentration in a monetary network produces centralization pressure that eventually compromises decentralization.

---

## 11.2 Token Parameters

| Parameter | Value |
|---|---|
| Token name | RupeeCoin |
| Ticker symbol | RPC |
| Total maximum supply | 1,000,000,000 RPC (1 billion) |
| Decimals | 8 (smallest unit: 1 satoshi-equivalent = 0.00000001 RPC) |
| Genesis supply | 0 (no pre-mine) |
| Initial circulating supply | 0 |

The maximum supply of 1 billion RPC is chosen to:
- Provide sufficient granularity for micro-transactions at the satoshi level.
- Avoid the psychological and practical complexity of Bitcoin's 21 million supply with 8 decimals.
- Support a population-scale user base without requiring sub-satoshi accounting.

---

## 11.3 Supply Emission Schedule

Supply enters circulation exclusively through validator block rewards.  
There is no pre-mine, no ICO allocation, no team reserve, and no foundation treasury pre-allocation.

### 11.3.1 Emission Phases

**Phase 1 — Bootstrap Emission (Years 1–4)**  
Block reward: 50 RPC per block  
Blocks per year: ~7,884,000 (4-second slots, 32-slot epochs, accounting for missed slots at 95% uptime)  
Annual emission: ~394,200,000 RPC  
*Note: Actual Year 1 emission will be lower as the validator set grows from genesis.*

**Phase 2 — Growth Emission (Years 5–8)**  
Block reward: 25 RPC per block  
Annual emission: ~197,100,000 RPC

**Phase 3 — Maturity Emission (Years 9–12)**  
Block reward: 12.5 RPC per block  
Annual emission: ~98,550,000 RPC

**Phase 4 — Consolidation Emission (Years 13–16)**  
Block reward: 6.25 RPC per block  
Annual emission: ~49,275,000 RPC

**Phase 5 and beyond — Tail Emission**  
After Year 16, block rewards transition to a fixed tail emission of 1 RPC per block.  
Annual tail emission: ~7,884,000 RPC (~0.79% of maximum supply annually).

The tail emission serves two purposes:
1. It ensures validators always have a block reward component, supplementing transaction fees.
2. It provides a modest, predictable inflation rate that compensates for lost coins (wallets with lost keys, etc.).

The tail emission does not push supply beyond the maximum cap.  
Once the circulating supply reaches 1,000,000,000 RPC, tail emission is suspended until supply falls below the cap due to fee burning (see Section 11.5).

### 11.3.2 Cumulative Supply Projection

| Year | Approximate Circulating Supply | % of Maximum |
|---|---|---|
| 1 | ~100,000,000 | 10% |
| 4 | ~400,000,000 | 40% |
| 8 | ~600,000,000 | 60% |
| 12 | ~750,000,000 | 75% |
| 16 | ~850,000,000 | 85% |
| 20+ | Approaching 1,000,000,000 | ~100% |

*These are approximations. Actual supply depends on validator participation rate and missed blocks.*

---

## 11.4 The Perpetual Validator Incentive — Solving Bitcoin's 2140 Problem

Bitcoin's long-term security depends on an unproven assumption: that transaction fees alone will sustain sufficient mining incentive after block rewards reach zero.

If fees are insufficient, miners exit. Hash rate drops. The cost of a 51% attack falls.  
This is not a theoretical problem — it is a structural flaw that will manifest on a known timeline.

RupeeCoin solves this with the **Perpetual Micro-Fee Pool (PMFP)**.

### 11.4.1 Mechanism

Every transaction on the RupeeCoin network pays a protocol fee.  
The protocol fee has two components:

**Validator fee (70%).**  
Paid directly to the validator that includes the transaction in a block.  
This is the standard transaction fee, analogous to Bitcoin's transaction fees.

**Protocol pool contribution (30%).**  
A fraction of every transaction fee is deposited into the Perpetual Micro-Fee Pool — a protocol-level smart contract that accumulates over time.

The PMFP distributes accumulated funds to active validators over a rolling 365-day window,  
proportional to their effective weight (stake × contribution score).

### 11.4.2 Economic Properties

The PMFP creates a smoothed, predictable validator income stream that does not depend on any single block's transaction volume.

At low network activity, the pool distributes slowly but continuously.  
At high network activity, the pool accumulates rapidly and distributes more.  
The 365-day distribution window smooths out periods of low activity.

Even in a scenario where block rewards reach zero and daily transaction volume drops to a minimum,  
the PMFP provides a floor of validator compensation drawn from historical accumulated fees.

The PMFP balance is transparent and auditable on-chain.  
Validators and token holders can observe the pool balance at any time.

### 11.4.3 Why 30%

The 30/70 split between pool and direct validator fees is a starting parameter, adjustable by governance.

The objective is:
- Pool contribution high enough to build a meaningful reserve during high-activity periods.
- Direct validator fee high enough to immediately compensate validators for including transactions.

At a network processing 10,000 transactions per second with an average fee of 0.001 RPC per transaction:  
Daily transaction fees: 10,000 × 86,400 × 0.001 = 864,000 RPC/day  
Daily pool contribution: 259,200 RPC/day  
Annual pool accumulation: ~94,608,000 RPC/year

At these volumes, the PMFP provides substantial validator incentive independent of block rewards.

---

## 11.5 Fee Burning

To create deflationary pressure during periods of high network activity,  
RupeeCoin implements a base fee burn mechanism.

### 11.5.1 Mechanism

Inspired by Ethereum's EIP-1559 but designed specifically for RupeeCoin's fee structure:

Each transaction pays:
- **Base fee** — a protocol-determined minimum fee, dynamically adjusted based on network congestion.
- **Priority fee** — an optional tip to validators for faster inclusion.

The base fee is **burned** — permanently removed from supply.  
The priority fee goes entirely to the validator.  
The 30% PMFP contribution is taken from the base fee before burning.

```
transaction_fee = base_fee + priority_fee

base_fee allocation:
  30% → Perpetual Micro-Fee Pool
  70% → burned (supply reduction)

priority_fee allocation:
  100% → block proposer validator
```

### 11.5.2 Dynamic Base Fee

The base fee adjusts automatically each block based on the previous block's utilization:

- If the previous block was more than 50% full: base fee increases by up to 12.5%.
- If the previous block was less than 50% full: base fee decreases by up to 12.5%.
- Target block utilization: 50%.

This creates fee predictability — users can estimate the base fee for the next block with high accuracy.  
It prevents fee spikes that priced small transactions out of the Ethereum network during high-demand periods.

### 11.5.3 Net Supply Effect

The fee burn mechanism means that at high network activity, supply contracts.  
Combined with the tail emission, the long-term supply equilibrium is determined by:

`net emission = tail emission - fee burn`

At sufficient network activity, fee burn exceeds tail emission and supply decreases.  
At low network activity, tail emission slightly increases supply.

This creates a supply mechanism that responds to actual network usage rather than a fixed schedule.

---

## 11.6 Initial Distribution — Fair Launch

RupeeCoin launches with zero pre-mined supply.  
The genesis block contains no allocations to founders, investors, or the development team.

### 11.6.1 Why No Pre-Mine

A pre-mine — allocating tokens to founders before public launch — creates:
- An immediate, permanent wealth advantage for insiders.
- Sell pressure from insiders who received tokens at zero cost.
- A trust deficit that is visible in the token's entire history.
- Regulatory exposure as a potential unregistered securities offering.

The protocols that have survived and grown over decades — Bitcoin foremost among them — launched without pre-mines.  
Satoshi's coins were earned through mining, not allocated before genesis.

RupeeCoin takes the same position.

### 11.6.2 Development Funding

The absence of a pre-mine requires an alternative mechanism for protocol development funding.

RupeeCoin uses a **Protocol Development Fund (PDF)** — a governance-controlled on-chain treasury funded by a fraction of block rewards during the bootstrap phase.

During Phase 1 (Years 1–4):  
5% of each block reward is directed to the Protocol Development Fund.  
At 50 RPC per block with 5% allocation: 2.5 RPC per block → PDF.  
At 95% validator uptime: ~19,710,000 RPC allocated to PDF over Phase 1.

The PDF is governed entirely by on-chain governance (see Section 14).  
No individual — including the founding team — can unilaterally access PDF funds.  
All expenditures require a governance vote.

The founding team may submit proposals for development funding through the governance process,  
on equal terms with any other contributor.

### 11.6.3 Genesis Validator Bootstrapping

At mainnet launch, the initial validator set must be established.  
RupeeCoin bootstraps the validator set through a public validator registration period prior to genesis:

1. Any party meeting the minimum stake requirement can register as a genesis validator.
2. Registration requires staking 10,000 RPC acquired through the testnet faucet or the public sale (if any).
3. Genesis validators are selected randomly from all registered validators up to the initial cap.

The genesis validator set is deliberately limited to ~500 validators at launch,  
expanding as the network matures and the validator queue activates additional participants.

---

## 11.7 Token Utility

RPC has three distinct utility functions within the protocol:

**1. Transaction fees.**  
All transactions on Layer 1 and Layer 2 require RPC for fees.  
This creates fundamental demand proportional to network usage.

**2. Staking.**  
Validators must stake RPC to participate in consensus.  
Staked RPC is locked and not available for circulation, creating supply constraint proportional to validator participation.

**3. Governance.**  
RPC holders participate in protocol governance (see Section 14).  
Governance participation requires holding RPC in a non-custodial wallet — not staked, to separate validator governance from holder governance.

---

## 11.8 Economic Security Model

The economic security of RupeeCoin — the cost an attacker must pay to compromise consensus — scales with:

1. The total value of staked RPC (attack requires acquiring 1/3 of staked supply).
2. The market price of RPC (higher price = higher attack cost).
3. The slashing mechanism (successful attack evidence results in stake destruction).

These three factors create a feedback loop:
- Greater network adoption → greater transaction volume → greater fee revenue → greater validator rewards → greater staking participation → higher attack cost.
- Higher attack cost → greater security → greater trust → greater adoption.

The tokenomics are designed to initiate and sustain this virtuous cycle.

---

## 11.9 Summary

| Parameter | Value |
|---|---|
| Maximum supply | 1,000,000,000 RPC |
| Pre-mine | None |
| Founder allocation | None |
| VC allocation | None |
| Phase 1 block reward | 50 RPC |
| Halving schedule | Every 4 years |
| Tail emission | 1 RPC per block (post-Phase 4) |
| Base fee burn | 70% of base fee |
| PMFP contribution | 30% of base fee |
| Protocol Development Fund | 5% of block rewards (Phase 1–2) |
| Minimum validator stake | 10,000 RPC |
| Unstaking delay | 28 days |

The monetary policy of RupeeCoin is defined entirely in this document and enforced by protocol code.  
It is not subject to discretionary adjustment by any individual or organization.  
Changes to monetary policy require a supermajority governance vote with extended time-lock (see Section 14).
