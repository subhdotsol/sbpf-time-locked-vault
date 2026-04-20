
; TIME LOCKED PDA VAULT (with initialize and withdraw function)
; vault account memory layout 

.equ VAULT_OWNER,     0x0000   ; 32 bytes (Pubkey)
.equ VAULT_UNLOCK,    0x0020   ; u64 (slot/time)
.equ VAULT_WITHDRAWN, 0x0028   ; u8  (0 or 1)


; Instruction ids
.equ IX_INIT,     0x00
.equ IX_WITHDRAW, 0x01


; entry point of the program
entrypoint:

    ; r1 = instruction data pointer

    ldr r2, [r1]          ; load instruction id

    cmp r2, IX_INIT
    je initialize

    cmp r2, IX_WITHDRAW
    je withdraw

    exit 1


; INITIALIZE VAULT (create state inside the PDA)
;
; Accounts:
;   r0[0] = vault PDA account (writable)
;   r0[1] = user (signer)
;   r1    = instruction data (unlock slot)

initialize:

    ; vault data pointer
    ldr r3, [r0 + 0x60]        ; VAULT_DATA pointer
    ; user pubkey
    ldr r4, [r0 + 0x10]        ; OWNER_KEY from user account


    ; store owner in vault (32 bytes)
    memcpy r3 + VAULT_OWNER, r4, 32

    ; store unlock slot (u64 from instruction)
    ; instruction_data[8..16]
    ldr r5, [r1 + 0x08]
    store64 r3 + VAULT_UNLOCK, r5

    ; set withdrawn = 0
    store8 r3 + VAULT_WITHDRAWN, 0

    exit 0

; WITHDRAW FLOW
;
; Accounts:
;   r0[0] = vault PDA (writable)
;   r0[1] = user (signer)
;   r0[2] = clock sysvar
;   r0[3] = destination account
withdraw:


    ; load vault data pointer
    ldr r3, [r0 + 0x60]

    ; read vault fields
    ldr r4, r3 + VAULT_OWNER       ; owner pubkey
    ldr r5, r3 + VAULT_UNLOCK      ; unlock slot
    ldr r6, r3 + VAULT_WITHDRAWN   ; withdrawn flag

    ; check: already withdrawn?
    cmp r6, 1
    je fail

    ; verify signer == owner
    ldr r7, [r0 + 0x10]        ; signer pubkey

    cmp_mem r4, r7, 32
    jne fail


    ; get current slot from clock sysvar
    ldr r8, [r0 + 0x120]       ; clock account data ptr
    ldr r9, r8 + 0x00          ; current slot (u64)


    ; check time lock
    cmp r9, r5
    jl fail


    ; transfer lamports
    ldr r10, [r0 + 0x50]       ; vault lamports
    ldr r11, [r0 + 0x70]       ; destination lamports

    add r11, r10
    mov r10, 0

    str r11, [r0 + 0x70]
    str r10, [r0 + 0x50]



    ; mark withdrawn = 1
    store8 r3 + VAULT_WITHDRAWN, 1

    exit 0

; FAIL EXIT
fail:
    exit 1