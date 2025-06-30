/* SPDX-License-Identifier: MPL-2.0 */
/*
 * LoongArch 初始引导（BSP / AP 共用）                 ────────────────
 * 入口寄存器约定：
 *   a0 = cpu_id
 *   a1 = BSP: DTB 物理地址 / AP: 栈顶虚拟地址（由 BSP 通过 IPI 传递）
 *
 *  ┌──────────────┐
 *  │  _start      │ BSP：固定引导栈
 *  ├──────────────┤
 *  │  _start_ap   │ AP ：BSP 传入栈
 *  └──────────────┘
 *           │
 *           ▼
 *    common_init → loongarch_boot (Rust)
 */

.macro CSR_WRITE reg,val
    li.d  \reg, \val
    csrwr \reg, \val
.endm

.section .text.entry
.global _start
.global _start_ap

/* ---------- BSP 入口 ---------- */
_start:
    la.global $sp, boot_stack_top       # 使用静态引导栈
    
    /* BSP 清空 .bss */
    la.global $t0, __bss
    la.global $t1, __bss_end
    li.d    $t2, 0x00000000ffffffff # 低 32 位物理地址掩码
    and     $t0, $t0, $t2
    and     $t1, $t1, $t2
0:  beq     $t0, $t1, common_init
    st.d    $zero, $t0, 0
    addi.d  $t0, $t0, 8
    b       0b

/* ---------- AP 入口 ---------- */
_start_ap:
    move    $sp, $a1                   # 栈顶由 BSP 传入
    b       common_init

/* ---------- 公共初始化 ---------- */
common_init:
    li.d    $t2, 0x00000000ffffffff # 低 32 位物理地址掩码
    /* DMW0: 0x8000‥ uncached, DMW1: 0x9000‥ cached + coherent */
    li.d    $t0, (0x8000000000000001)
    csrwr   $t0, 0x180                # CSR_DMWIN0
    li.d    $t0, (0x9000000000000011) # MAT=CA | PLV0
    csrwr   $t0, 0x181                # CSR_DMWIN1

    /* Page-table walker 描述（Sv48 固定布局） */
    li.d    $t0, ((0 << 30) | 12 | (9 << 5) | (21 << 10) | (9 << 15) | (30 << 20) | (9 << 25))  # PTE 8B + 各层偏移/宽度
    csrwr   $t0, 0x1c               # CSR_PWCL
    li.d    $t0, (39 | (9 << 6))    # Dir3 base / width
    csrwr   $t0, 0x1d               # CSR_PWCH

    /* 统一配置 TLB/SLTB 页大小 = 4KiB */
    li.d    $t0, 12
    csrwr   $t0, 0x10               # CSR_TLBIDX.PS
    csrwr   $t0, 0x1e               # CSR_STLBPS.PS
    csrwr   $t0, 0x8e               # CSR_TLBREHI.PS

    /* 临时 TLB refill 向量 (4KiB 对齐，物理地址) */
    la.global $t0, _tlb_fill
    and     $t0, $t0, $t2
    li.d    $t3, 0x9000000000000000
    or      $t0, $t0, $t3
    csrwr   $t0, 0x88               # CSR_TLBRENTRY

    /* ------------------ 构造引导页表 ------------------ */
    la.global $t0, boot_pagetable
    la.global $t1, boot_pagetable_1st
    la.global $t4, boot_pagetable_2nd
    and     $t0, $t0, $t2
    and     $t1, $t1, $t2
    and     $t4, $t4, $t2

    /* Common flags for non‑leaf directory entries:
     * 0x001 = _PAGE_VALID
     * 0x080 = _PAGE_PRESENT
     * 0x100 = _PAGE_WRITE (harmless but convenient)
     * 0x010 = _CACHE_CC  (coherent cached)
     *            ----
     *        => 0x191
     */
    li.d    $t6, 0x0000000000000193   # DirAttr = VAL|PRES|WRITE|CC

    /* ---------- L0[256] → boot_pagetable_1st ---------- */
    li.d    $t3, 256*8          # 2048 → 放进寄存器
    add.d   $t3, $t0, $t3         # t3 = base + 2048
    or      $t5, $t1, $t6          # PPN | attributes
    st.d    $t5,  $t3, 0         # [256] = phys(boot_pagetable_1st)

    /* ---------- L0[511] → boot_pagetable_2nd ---------- */
    li.d    $t3, 511*8          # 4088 → 放进寄存器
    add.d   $t3, $t0, $t3         # t3 = base + 4088
    or      $t5, $t4, $t6          # PPN | attributes
    st.d    $t5,  $t3, 0         # [511] = phys(boot_pagetable_2nd)

    /* ------------------ 打开分页 ------------------ */
    csrwr   $t0, 0x1a               # CSR_PGDH (VA[47]=1)
    csrwr   $t0, 0x19            # CSR_PGDL (VA[47]=0)
    invtlb  0x00, $zero, $zero

    li.w		 $t0, 0xb0		              # PLV=0, IE=0, PG=1
    csrwr	  	 $t0, 0x0                     # LOONGARCH_CSR_CRMD
    li.w	 	 $t0, 0x00		              # PLV=0, PIE=0, PWE=0
    csrwr		 $t0, 0x1                     # LOONGARCH_CSR_PRMD
    li.w		 $t0, 0x00		              # FPE=0, SXE=0, ASXE=0, BTE=0
    csrwr		 $t0, 0x2                     # LOONGARCH_CSR_EUEN

    /* ------------------ 跳入 Rust ------------------ */
    li.d    $t1, 0xffffffff00000000 # 内核高半偏移
    la.global $t0, loongarch_boot
    or      $t0,  $t0, $t1
    jirl    $zero, $t0, 0           # (a0 = cpu_id, a1 保持原意)

 /* ===== TLB refill ===== */
.section .text
.balign 4096
.global tmp_tlb_refill
tmp_tlb_refill:
    .equ LA_CSR_PGDL,          0x19    /* Page table base address when VA[47] = 0 */
    .equ LA_CSR_PGDH,          0x1a    /* Page table base address when VA[47] = 1 */
    .equ LA_CSR_PGD,           0x1b    /* Page table base */
    .equ LA_CSR_TLBRENTRY,     0x88    /* TLB refill exception entry */
    .equ LA_CSR_TLBRBADV,      0x89    /* TLB refill badvaddr */
    .equ LA_CSR_TLBRERA,       0x8a    /* TLB refill ERA */
    .equ LA_CSR_TLBRSAVE,      0x8b    /* KScratch for TLB refill exception */
    .equ LA_CSR_TLBRELO0,      0x8c    /* TLB refill entrylo0 */
    .equ LA_CSR_TLBRELO1,      0x8d    /* TLB refill entrylo1 */
    .equ LA_CSR_TLBREHI,       0x8e    /* TLB refill entryhi */
    .globl _tlb_fill
    .align 12
    _tlb_fill:
        csrwr   $t0, LA_CSR_TLBRSAVE
        csrrd   $t0, LA_CSR_PGD
        lddir   $t0, $t0, 3
        beqz    $t0, _break
        lddir   $t0, $t0, 2
        beqz    $t0, _break
        lddir   $t0, $t0, 1
        beqz    $t0, _break
        ldpte   $t0, 0
        ldpte   $t0, 1
        tlbfill
        csrrd   $t0, LA_CSR_TLBRSAVE
        ertn
    _break:
        csrwr   $zero, LA_CSR_TLBRELO0
        csrwr   $zero, LA_CSR_TLBRELO1
        tlbfill
        csrrd   $t0, LA_CSR_TLBRSAVE
        ertn

/* ===== Boot stack（64 KiB）===== */
.section .bss.stack
.global boot_stack_bottom
boot_stack_bottom:
    .space 0x40000
.global boot_stack_top
boot_stack_top:

/* ===== 引导页表 ===== */
.section .data

.align 12
boot_pagetable:
    .quad (0x00000 << 12) | 0x0000000000001c3 # PVDWH | HUGE | R/W/X/D/A
    .zero 8 * 255
    .quad 0  # [256] 指向第一个子页表 boot_pagetable_1st
    .zero 8 * 254
    .quad 0  # [511] 指向第二个子页表 boot_pagetable_2nd

# 第一个子页表
boot_pagetable_1st:
    # 映射低地址空间
    .quad (0x00000 << 12) | 0x0000000000001c3 # PVDWH | HUGE | R/W/X/D/A
    .quad (0x40000 << 12) | 0x0000000000001c3 # PVDWH | HUGE | R/W/X/D/A
    .quad (0x80000 << 12) | 0x0000000000001c3 # PVDWH | HUGE | R/W/X/D/A
    .zero 8 * 509

# 第二个子页表
boot_pagetable_2nd:
    # 映射低地址空间
    .zero 8 * 508
    .quad (0x00000 << 12) | 0x0000000000001c3 # PVDWH | HUGE | R/W/X/D/A
    .quad (0x40000 << 12) | 0x0000000000001c3 # PVDWH | HUGE | R/W/X/D/A
    .quad (0x80000 << 12) | 0x0000000000001c3 # PVDWH | HUGE | R/W/X/D/A
    .quad 0
