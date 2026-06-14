# Section 14 — Governance

> A decentralized protocol without governance is frozen.  
> A decentralized protocol with poorly designed governance is captured.  
> The objective is a governance system that allows the protocol to evolve  
> while making capture, corruption, and unilateral control structurally impossible.  
> This section defines how RupeeCoin governs itself.

---

## 14.1 The Governance Problem

Every long-lived protocol must change.  
Cryptographic standards become obsolete. Performance bottlenecks are discovered. Economic parameters require tuning.  
New attack vectors emerge. The world changes in ways the designers did not anticipate.

Bitcoin's approach to governance — informal, off-chain, consensus-driven — has produced a protocol that is extremely resistant to change.  
This is both its greatest strength and its greatest limitation.  
Bitcoin cannot easily fix its quantum vulnerability because there is no mechanism for coordinated migration.  
It cannot adjust its block size because every change requires a contentious political battle with no formal resolution mechanism.

Ethereum's approach — more structured, with formal EIP processes and a more active core development team — has produced a more adaptable protocol but one where the influence of the core team and the Ethereum Foundation is substantial and informal.

RupeeCoin requires a governance system that is:

- **Formal** — decisions are made through defined processes, not informal consensus.
- **On-chain** — governance actions are executed by protocol code, not by individuals.
- **Resistant to capture** — no individual, team, or corporation can dominate governance.
- **Resistant to paralysis** — the protocol can make necessary changes in reasonable timeframes.
- **Transparent** — all governance activity is publicly observable and auditable.
- **Time-locked** — major changes allow the community time to respond, including exit.

---

## 14.2 Governance Participants

Three classes of participants have governance rights in RupeeCoin:

### 14.2.1 Token Holders

Any address holding RPC in a non-custodial wallet can participate in governance.  
Governance weight is proportional to RPC balance at the time of the vote snapshot.  
Tokens held on exchanges or in custodial services do not have governance rights — only self-custodied tokens.

This creates an incentive for holders to self-custody,  
which aligns with the protocol's broader goal of self-sovereignty.

### 14.2.2 Validators

Active validators have additional governance weight derived from their staked balance and contribution score.  
Validator governance weight: `staked_balance × contribution_multiplier × 1.5`

The 1.5 multiplier reflects that validators have a greater economic stake in protocol correctness —  
their staked tokens are at risk of slashing, creating stronger alignment with network health.

Validators that are slashed lose their governance weight immediately.

### 14.2.3 Protocol Development Fund Stewards

A committee of 7 stewards is elected by token holders to administer PDF disbursements.  
Stewards have no authority over protocol parameters — only over PDF fund allocation.  
Steward elections occur annually. Any token holder meeting a minimum stake threshold can stand for election.  
Stewards serve one-year terms with a maximum of three consecutive terms.

---

## 14.3 Proposal Types

Governance proposals are categorized by their impact on the protocol.  
Each category has different thresholds, voting periods, and time-lock durations.

### Category 1 — Parameter Adjustments
*Low-impact changes to protocol parameters within predefined safe ranges.*

Examples:
- Adjusting base fee target utilization (within ±20% of current value)
- Adjusting contribution score weights (within ±10% of current values)
- Adjusting the PMFP distribution window (within defined bounds)
- Updating the validator set size limit (within ±20%)

Approval threshold: Simple majority (>50%) of participating governance weight  
Minimum participation: 5% of total governance weight  
Voting period: 7 days  
Time-lock after approval: 7 days  
Implementation: Automatic on-chain execution after time-lock

### Category 2 — Protocol Upgrades
*Significant changes to protocol behavior, new features, or non-trivial parameter changes.*

Examples:
- Introducing new transaction types
- Upgrading the ZK-proof system
- Modifying the Layer 2 architecture
- Changing the validator selection algorithm
- Adjusting reward distribution mechanics outside Category 1 bounds

Approval threshold: Supermajority (>66%) of participating governance weight  
Minimum participation: 10% of total governance weight  
Voting period: 14 days  
Time-lock after approval: 28 days  
Implementation: Automatic on-chain execution after time-lock

### Category 3 — Constitutional Changes
*Fundamental changes to the protocol's core properties.*

Examples:
- Changes to the maximum supply
- Changes to the monetary emission schedule
- Changes to the governance mechanism itself
- Changes to the slashing conditions
- Additions of new cryptographic primitives to the core protocol

Approval threshold: Supermajority (>75%) of participating governance weight  
Minimum participation: 20% of total governance weight  
Voting period: 30 days  
Time-lock after approval: 90 days  
Implementation: Automatic on-chain execution after time-lock  
Additional requirement: No single entity may cast more than 5% of total governance weight on a Category 3 vote.

### Category 4 — Emergency Actions
*Critical security responses requiring rapid protocol action.*

Examples:
- Halting a compromised contract
- Emergency patch for an active exploit
- Temporary parameter adjustments to respond to an attack

Approval threshold: Supermajority (>75%) of validator governance weight only  
(Token holder voting is suspended for emergency actions — validators are online and can respond in time.)  
Minimum participation: 50% of active validator governance weight  
Voting period: 24 hours  
Time-lock after approval: None (immediate execution)  
Expiry: Emergency actions automatically expire after 14 days unless ratified through a Category 2 vote.

---

## 14.4 The Proposal Process

### 14.4.1 Request for Comment (RFC)

Before a formal proposal is submitted, the proposer is expected — but not required — to publish an RFC on the protocol's public governance forum.

An RFC includes:
- Problem statement
- Proposed solution
- Technical specification
- Economic impact analysis
- Security considerations
- Alternative approaches considered

The RFC period allows community discussion and refinement before committing to a formal vote.  
Proposals that arrive as formal votes without prior RFC may face lower participation due to lack of familiarity.

### 14.4.2 Formal Proposal Submission

A formal proposal is submitted on-chain by an address meeting the minimum proposal stake requirement:

- Category 1: 1,000 RPC
- Category 2: 10,000 RPC
- Category 3: 50,000 RPC
- Category 4: Must be submitted by an active validator

The proposal stake is returned after the vote regardless of outcome,  
unless the proposal is found to be malformed or submitted in bad faith (as determined by a subsequent governance vote).

The proposal must include:
- Category classification
- Human-readable description
- Technical specification of on-chain changes
- Proposed implementation commit hash (for code changes)

### 14.4.3 Voting

Voting occurs on-chain.  
Each governance participant casts their vote — Yes, No, or Abstain — signed with their wallet key.

Abstain votes count toward the participation minimum but not toward the approval threshold.  
This allows participants to signal engagement without taking a position.

Vote delegation is supported:  
a token holder can delegate their governance weight to another address for a defined period.  
Delegation is revocable at any time.  
Validators cannot accept delegation of token holder votes — this prevents validators from accumulating disproportionate governance power.

### 14.4.4 Implementation

Approved proposals are implemented automatically by the protocol after the time-lock expires.  
No individual or team has the ability to veto an approved proposal or delay its implementation beyond the defined time-lock.  
No individual or team has the ability to implement a change that has not been approved through governance.

This is enforced at the protocol level, not by social convention.

---

## 14.5 Anti-Capture Mechanisms

Governance capture — the accumulation of sufficient governance power by a single actor to control protocol decisions — is the primary governance threat.

RupeeCoin implements multiple independent mechanisms to resist capture:

### 14.5.1 Voting Weight Cap

No single address may exercise more than **5% of total governance weight** on any Category 3 vote.  
Weight above 5% is not counted — it is not redistributed, it is simply excluded.

For Category 1 and 2 votes, the cap is 10%.

This prevents a whale — an exchange, a large holder, or the founding team — from unilaterally controlling constitutional decisions even if they hold a large fraction of supply.

### 14.5.2 Exchange Exclusion

Tokens held in custodial addresses — identified through a combination of on-chain analysis and voluntary exchange registration — do not have governance rights.

Exchanges holding RPC on behalf of their customers cannot vote with those tokens.  
This prevents exchanges from wielding their aggregate custody balance as governance power without the consent of the actual token holders.

### 14.5.3 Time-Locks as Exit Windows

The time-lock after approval of Category 2 (28 days) and Category 3 (90 days) proposals is not merely a technical delay.  
It is an exit window.

A token holder who strongly disagrees with an approved proposal has time to exit the protocol before the change takes effect.  
This is a fundamental property of a fair monetary system:  
participants can leave before rules they find unacceptable are imposed on them.

The exit window creates a natural check on extreme proposals:  
a change that causes significant capital flight before implementation signals the community's rejection even after a formal approval.

### 14.5.4 Founder Privilege Removal

At protocol launch, the founding team holds no special governance privileges.  
They participate in governance as token holders and validators — on identical terms to any other participant.

Any tokens earned by founders through validator rewards are subject to the same vesting and governance rules as all other tokens.  
Founder validator keys are not granted any special status.

The founding team commits to a governance participation policy:  
combined founder governance weight will not exceed 5% of total governance weight in any vote,  
regardless of actual token holdings.

This commitment is enforced socially, not by protocol code, during the protocol's early years.  
After Phase 1 (Year 4), the governance cap mechanism (Section 14.5.1) enforces it at the protocol level.

### 14.5.5 Minimum Participation Requirements

Proposals that do not achieve minimum participation thresholds fail regardless of the Yes/No ratio.  
This prevents a small coordinated group from passing proposals during periods of low engagement.

A quorum requirement ensures that governance decisions reflect broad community engagement,  
not the preferences of the most active minority.

---

## 14.6 Protocol Development Fund Governance

The Protocol Development Fund (PDF) is governed by the elected steward committee under the following rules:

**Disbursement proposals:**  
Any address can submit a PDF disbursement proposal with a description of work and a requested amount.  
Disbursement proposals require approval by 5 of 7 stewards.  
Steward votes are public and on-chain.

**Disbursement limits:**  
No single disbursement may exceed 500,000 RPC without a Category 2 governance vote.  
The steward committee may not approve more than 2,000,000 RPC in disbursements per month without a governance vote.

**Steward accountability:**  
Steward voting records are permanently public.  
Any steward that approves a disbursement later found to be fraudulent or contrary to protocol interests can be removed by a Category 1 governance vote.  
Removed stewards are replaced by election within 30 days.

**PDF transparency:**  
The PDF balance, all incoming contributions, and all disbursements are permanently recorded on-chain and publicly auditable.

---

## 14.7 Governance Limitations

There are categories of protocol change that governance cannot make, regardless of vote outcome.

**Governance cannot:**

1. **Increase the maximum supply beyond 1,000,000,000 RPC.**  
   The maximum supply cap is not a governance parameter. It is hardcoded.  
   A change to the maximum supply would require a hard fork — a new chain.

2. **Remove the slashing mechanism.**  
   Slashing is a core security property. Its removal would eliminate the economic deterrent against malicious validator behavior.

3. **Grant any address immunity from protocol rules.**  
   No address — including founder addresses — can be granted exemption from fees, slashing, or governance weight caps.

4. **Implement backdoors for regulatory access.**  
   The protocol does not implement forced disclosure of private transactions.  
   Users may choose selective disclosure voluntarily. They cannot be compelled by protocol code.

5. **Transfer governance control to a single entity.**  
   A vote to make governance decisions by committee, by a single address, or by any non-decentralized mechanism is invalid by protocol rule.

These hardcoded constraints are the constitutional bedrock of RupeeCoin.  
They cannot be changed without forking the chain — which requires the voluntary adoption of the fork by the community.

---

## 14.8 The Endgame — Full Protocol Independence

RupeeCoin's governance roadmap has an explicit endpoint: **full protocol independence from its founders**.

**Phase 0–1 (Years 0–2):**  
The founding team maintains elevated informal influence through code contributions and community leadership.  
No formal governance privileges. All governance participation on equal terms.

**Phase 2–3 (Years 2–4):**  
On-chain governance fully active.  
Founding team governance participation policy (≤5% weight) publicly committed and monitored.  
PDF steward elections fully community-controlled.

**Phase 4 (Year 4):**  
Protocol Development Fund founder contribution period ends.  
Founding team formally relinquishes any remaining informal governance conventions.  
Protocol is governed entirely by its community through on-chain mechanisms.

**Phase 10 (Year 5+):**  
All governance weight caps enforced at protocol level.  
No entity — including any entity that could be associated with the founding team — holds more than 5% governance weight.  
The protocol is owned by no one and governed by everyone.

This is not a goal. It is a commitment.  
The governance mechanism is designed from day one to make this outcome structurally inevitable.

---

## 14.9 Summary

| Aspect | Design |
|---|---|
| Governance model | On-chain, token-weighted, validator-amplified |
| Proposal categories | 4 (Parameter / Upgrade / Constitutional / Emergency) |
| Constitutional approval threshold | 75% supermajority, 20% quorum |
| Maximum single-entity weight (Cat. 3) | 5% |
| Exchange custody exclusion | Yes |
| Time-lock (Constitutional) | 90 days |
| Founder special privileges | None |
| Hardcoded limits | Supply cap, slashing, no backdoors, no governance transfer |
| Endgame | Full community ownership, no founder control |

RupeeCoin's governance is designed to be the last line of defense against the protocol becoming what it was built to replace:  
a system controlled by a small group of powerful actors at the expense of everyone else.
