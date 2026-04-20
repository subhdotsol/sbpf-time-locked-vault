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
; Ix data offsets (after accounts):
;   initialize (2 accts): r9 + 0x08 + 2*0xC0 = r9 + 0x188
;   withdraw   (4 accts): r9 + 0x08 + 4*0xC0 = r9 + 0x308
;
; Vault data (in account[0].data):
;   +0x00  owner      Pubkey (32 bytes)
;   +0x20  unlock     u64
;   +0x28  withdrawn  u8


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

; Ix data base offsets (in input buffer)
.equ IX_DATA_INIT,      0x188   ; after 2 accounts: 0x08 + 2*0xC0
.equ IX_DATA_WITH,      0x308   ; after 4 accounts: 0x08 + 4*0xC0

; Instruction IDs
.equ IX_INIT,           0x00
.equ IX_WITHDRAW,       0x01


.globl entrypoint
entrypoint:
    ; Debug: log r1 (input buffer) and first few bytes
    mov64 r5, r1
    ldxdw r1, [r5 + 0x00]   ; num_accounts
    mov64 r2, 0x1111        ; marker
    mov64 r3, 0x2222        ; marker
    mov64 r4, 0x3333        ; marker
    call sol_log_64_

    ; Restore r1 from r5 (which was original r1)
    mov64 r1, r5
    ; Save input buffer ptr in r9 (callee-saved, survives syscalls)
    mov64 r9, r1

    ; Dispatch on ix_id.
    ; Try initialize ix_id offset first (2-account instruction)
    ldxb  r2, [r9 + IX_DATA_INIT]
    jeq   r2, IX_INIT, initialize

    ; Try withdraw ix_id offset (4-account instruction)
    ldxb  r2, [r9 + IX_DATA_WITH]
    jeq   r2, IX_WITHDRAW, withdraw

    mov64 r0, 1
    exit


; INITIALIZE
; Accounts: [0]=vault PDA (writable), [1]=user (signer)
; Ix data at r9+IX_DATA_INIT: [0]=0x00 ix_id, [8..15]=unlock_slot u64

initialize:
    ; r6 = vault data pointer
    ldxdw r6, [r9 + ACCT0 + ACCT_DATA_PTR]

    ; Copy user pubkey (account[1].key) → vault_data.owner
    ; sol_memcpy_(dst r1, src r2, len r3)
    mov64 r1, r6                    ; dst = vault_data + VAULT_OWNER (= r6+0)
    mov64 r2, r9
    add64 r2, ACCT1                 ; src = &account[1].key (offset 0x00 within acct)
    mov64 r3, 32
    call sol_memcpy_

    ; Store unlock_slot (u64 at ix_data+8)
    ldxdw r2, [r9 + IX_DATA_INIT + 8]
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

    ; Verify signer pubkey == vault owner (32-byte compare)
    ; sol_memcmp_(s1 r1, s2 r2, n r3, result_ptr r4)
    mov64 r1, r6                    ; r1 = vault_data.owner
    mov64 r2, r9
    add64 r2, ACCT1                 ; r2 = account[1].key (signer pubkey)
    mov64 r3, 32
    mov64 r4, r10
    add64 r4, -8                    ; r4 = stack slot for result
    mov64 r8, 0
    stxdw [r10 - 8], r8             ; zero the result slot
    call sol_memcmp_

    ; result != 0 means keys differ → fail
    ldxdw r2, [r10 - 8]
    jne   r2, 0, fail

    ; Read current slot from clock sysvar
    ; Clock sysvar data: [+0x00]=slot u64
    ldxdw r3, [r9 + ACCT2 + ACCT_DATA_PTR]
    ldxdw r4, [r3 + 0x00]          ; r4 = current slot

    ; Check time lock
    ldxdw r5, [r6 + VAULT_UNLOCK]  ; r5 = unlock slot
    jlt   r4, r5, fail             ; current < unlock → fail

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