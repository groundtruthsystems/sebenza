# Diagrams: ASCII in the Terminal, Mermaid in the Document (shared reference)

> Shared reference for all `sebenza-*` skills. No frontmatter — loaded on demand.

Every diagram Sebenza produces has **two renderings**:

| Rendering | Where | Purpose |
|-----------|-------|---------|
| **ASCII** | the terminal, during a draft → confirm loop | so the user can actually verify it |
| **Mermaid** | the written artifact (`design.md`, `<domain>-architecture.md`, `spec.md`) | so it renders in editors, IDEs, and forges |

**Never paste a ` ```mermaid ` block into chat.** A terminal does not render it, so the user is being
asked to approve markup they cannot read. Draw it in ASCII for confirmation; write Mermaid to the file.

## The rule

1. **Confirm in ASCII.** Draw the diagram as plain text, ≤ 80 columns, and ask for confirmation on
   *that*.
2. **Write Mermaid.** Once confirmed, write the equivalent ` ```mermaid ` block into the artifact and
   tell the user the file path and section.
3. **Keep the two in step.** If the user revises the ASCII sketch, update the Mermaid block to match
   before moving on. They must never disagree.
4. **When ASCII will not work, say so.** Some graphs are too dense for text (many crossing edges,
   >~12 nodes). Do not ship an unreadable tangle: fall back to the node/edge summary below, state that
   the Mermaid version in the document is authoritative, and point the user at it.

Prefer plain ASCII (`+ - | < > v ^`) over box-drawing characters — it survives every terminal, font,
and copy-paste. Keep labels short; detail belongs in the narrative, not the sketch.

## Patterns by diagram type

### Use case (actors → use cases)

```
  Actors                     Use Cases
  ------                     ---------
  Clinician   ---->  [ UC1  Review a flagged claim ]
              ---->  [ UC2  Override a denial ]
  Biller      ---->  [ UC2  Override a denial ]
              ---->  [ UC3  Export the audit trail ]
  Scheduler*  ---->  [ UC4  Re-score claims nightly ]
                                        (* system actor, not a person)
```

### Activity / flow

```
  (start)
     |
     v
  [ Load claim ]
     |
     v
  < eligible? > -- no --> [ Mark ineligible ] --> (end)
     |
    yes
     v
  [ Score claim ]
     |
     v
  < score >= 0.8 ? > -- no --> [ Queue for human review ] --+
     |                                                      |
    yes                                                      |
     v                                                      |
  [ Auto-approve ]                                          |
     |                                                      |
     +---------------------------+--------------------------+
                                 v
                               (end)
```

### Class / domain entities

```
  +----------------------------+       +--------------------------+
  | Claim                      | 1   * | ReviewDecision           |
  +----------------------------+------>+--------------------------+
  | id: UUID                   |       | id: UUID                 |
  | patient_id: UUID    (PHI)  |       | claim_id: UUID           |
  | amount: Decimal            |       | outcome: Outcome         |
  | status: ClaimStatus        |       | reviewer_id: UUID        |
  +----------------------------+       +--------------------------+
  | score(): float             |       | apply(): void            |
  +----------------------------+       +--------------------------+
```

Mark PHI/PII fields inline — it is what the data and security domains will be checked against.

### Component / container

```
  +--------------------------------------------------+
  |  Claims Platform                                 |
  |                                                  |
  |   +-----------+          +------------------+    |
  |   | Web UI    |--HTTPS-->| Claims API       |    |
  |   +-----------+          +---------+--------+    |
  |                                    | gRPC        |
  |                          +---------v--------+    |
  |                          | Scoring Service  |    |
  |                          +---------+--------+    |
  +------------------------------------|-------------+
                                       | SQL
                               +-------v--------+
                               | Postgres       |
                               +----------------+
```

### Deployment / technology

```
  [ Region af-south-1 ]
    +-- ECS cluster "claims"
    |     +-- claims-api       x3   (2 vCPU / 4 GB)
    |     +-- scoring-worker   x2   (4 vCPU / 8 GB)
    +-- RDS Postgres 16             (multi-AZ, 100 GB gp3)
    +-- S3 "claims-exports"         (lifecycle: 90d -> Glacier)
```

### Capability map (business domain)

```
  Revenue Cycle
    +-- Claim Intake ............. unchanged
    +-- Claim Adjudication ....... CHANGED   (auto-scoring added)
    +-- Denial Management ........ CHANGED   (override flow)
    +-- Reporting ................ extended  (audit export)
```

### Data flow with trust boundaries (security domain)

```
  == Internet =====================================
     [ Clinician browser ]
              | TLS 1.3
  == DMZ ==================|=======================
     [ API Gateway ]  <-- authn: OIDC + MFA
              | mTLS
  == Private subnet =======|=======================
     [ Claims API ] ---> [ Postgres ]   AES-256 at rest, PHI
              |
              +---------> [ Audit log ] append-only, 7y retention
```

Boundaries as `==` rules are the point of this one — every arrow that crosses a boundary is a place
the security domain must state what authenticates and what encrypts.

### Coverage map (test plan)

```
  UC-01 Submit claim ------+--> TS-01 happy path ......... integration
                           +--> TS-02 missing payer ...... unit
                           +--> TS-03 duplicate claim .... integration

  UC-02 Override denial ---+--> TS-04 authorised override  e2e
                           +--> TS-05 unauthorised (403) . e2e   [threat T-03]

  R-02 scoring timeout ----+--> TS-06 degraded mode ...... integration
```

Optional — draw it only when the fan-out is worth seeing. The traceability **table** in
`test-plan.md` is mandatory and authoritative; this picture just makes a thin column of coverage
obvious at a glance. A use case with no arrow leaving it is the thing the map exists to expose.

## Fallback: too dense for ASCII

State the limit plainly, summarize structurally, and defer to the document:

```
  Nodes:  A Web UI | B Claims API | C Scoring | D Postgres | E Audit sink
  Edges:  A -HTTPS-> B ;  B -gRPC-> C ;  B -SQL-> D ;
          C -SQL-> D ;    B --> E ;      C --> E
  (+11 more edges — too dense to draw legibly here. The Mermaid diagram
   in application-architecture.md is authoritative.)
```
