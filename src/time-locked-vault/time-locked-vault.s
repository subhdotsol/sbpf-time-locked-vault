; TIME-LOCKED PDA VAULT

; Input buffer layout (r1 on entry):
;   [+0x00] num_accounts  u64
;   [+0x08] account[0]    0xC0 bytes  → base ACCT0 = 0x08
;   [+0xC8] account[1]    0xC0 bytes  → base ACCT1 = 0xC8
;   [+0x188] account[2]   0xC0 bytes  → base ACCT2 = 0x188
;   [+0x248] account[3]   0xC0 bytes  → base ACCT3 = 0x248
;
; Per-account offsets (relative to account base):
;   +0x00  key (pubkey, 32 bytes)
;   +0x40  lamports u64
;   +0x50  data_ptr *u8
;
; Instruction Data Layout (Loader v2):
;   [8 + num_accounts * 192]         data_len (u64)
;   [8 + num_accounts * 192 + 8]     data (actual ix bytes)
;
; Initialize (2 accounts): Offset to data = 8 + 2 * 192 + 8 = 400 (0x190)
; Withdraw   (4 accounts): Offset to data = 8 + 4 * 192 + 8 = 784 (0x310)

; Account bases
.equ ACCT0,             0x08
.equ ACCT1,             0xC8
.equ ACCT2,             0x188
.equ ACCT3,             0x248

; Per-account field offsets
.equ ACCT_KEY,          0x00
.equ ACCT_LAMPORTS,     0x40
.equ ACCT_DATA_PTR,     0x50

; Vault state offsets
.equ VAULT_OWNER,       0x00
.equ VAULT_UNLOCK,      0x20
.equ VAULT_WITHDRAWN,   0x28

; Instruction Data Start Offsets
.equ IX_START_INIT,     0x190   ; 0x08 + 2*192 + 8
.equ IX_START_WITH,     0x310   ; 0x08 + 4*192 + 8

; Instruction IDs
.equ IX_INIT,           0x00
.equ IX_WITHDRAW,       0x01

.globl entrypoint
entrypoint:
    ; Save input buffer ptr in r9
    mov64 r9, r1

    ; Read num_accounts to safely dispatch
    ldxdw r1, [r9 + 0x00]
    
    ; 2 accounts -> Initialize path
    jeq   r1, 2, initialize_check
    
    ; 4 accounts -> Withdraw path
    jeq   r1, 4, withdraw_check

    mov64 r0, 1 ; Unknown account count
    exit

initialize_check:
    ldxb  r2, [r9 + IX_START_INIT]
    jeq   r2, IX_INIT, initialize
    mov64 r0, 1
    exit

withdraw_check:
    ldxb  r2, [r9 + IX_START_WITH]
    jeq   r2, IX_WITHDRAW, withdraw
    mov64 r0, 1
    exit

; INITIALIZE
; Accounts: [0]=vault PDA, [1]=user (signer)
; Ix data: [0]=0x00, [1..8]=unlock_slot

initialize:
    ; r6 = vault data pointer
    ldxdw r6, [r9 + ACCT0 + ACCT_DATA_PTR]

    ; Copy user pubkey (account[1].key) → vault_data.owner
    mov64 r1, r6                    ; dst
    mov64 r2, r9
    add64 r2, ACCT1                 ; src = &account[1].key
    mov64 r3, 32
    call sol_memcpy_

    ; Store unlock_slot (u64 at IX_START_INIT + 1)
    ldxdw r2, [r9 + IX_START_INIT + 1]
    stxdw [r6 + VAULT_UNLOCK], r2

    ; Set withdrawn = 0
    mov64 r2, 0
    stxb  [r6 + VAULT_WITHDRAWN], r2

    mov64 r0, 0
    exit

; WITHDRAW
; Accounts: [0]=vault PDA, [1]=user (signer), [2]=clock sysvar, [3]=destination

withdraw:
    ; r6 = vault data pointer
    ldxdw r6, [r9 + ACCT0 + ACCT_DATA_PTR]

    ; Check withdrawn flag
    ldxb  r2, [r6 + VAULT_WITHDRAWN]
    jeq   r2, 1, fail

    ; Verify signer pubkey == vault owner
    mov64 r1, r6                    ; s1 = vault_data.owner
    mov64 r2, r9
    add64 r2, ACCT1                 ; s2 = account[1].key
    mov64 r3, 32
    mov64 r4, r10
    add64 r4, -8                    ; result_ptr
    stxdw [r10 - 8], r0             ; clear result slot
    call sol_memcmp_

    ldxdw r2, [r10 - 8]
    jne   r2, 0, fail

    ; Read current slot from clock sysvar
    ldxdw r3, [r9 + ACCT2 + ACCT_DATA_PTR]
    ldxdw r4, [r3 + 0x00]          ; r4 = current slot

    ; Check time lock
    ldxdw r5, [r6 + VAULT_UNLOCK]  ; r5 = unlock slot
    jlt   r4, r5, fail

    ; Transfer lamports: vault → destination
    ldxdw r7, [r9 + ACCT0 + ACCT_LAMPORTS]
    ldxdw r8, [r9 + ACCT3 + ACCT_LAMPORTS]
    add64 r8, r7
    mov64 r7, 0
    stxdw [r9 + ACCT3 + ACCT_LAMPORTS], r8
    stxdw [r9 + ACCT0 + ACCT_LAMPORTS], r7

    ; Mark withdrawn = 1
    mov64 r2, 1
    stxb  [r6 + VAULT_WITHDRAWN], r2

    mov64 r0, 0
    exit

fail:
    mov64 r0, 1
    exit