/* SPDX-License-Identifier: MPL-2.0 */

.section .text.entry
.globl _start
_start:
    # -- BSP 路径 --
    # 从SBI传递的参数：
    #   a0 = hart id
    #   a1 = DTB 物理地址

    # 设置 BSP 栈指针 (使用固定区域)
    lga     sp, boot_stack_top
    j       setup_paging    # 跳转到公共的分页设置

.globl _start_ap
_start_ap:
    # -- AP 路径 --
    # 从SBI传递的参数：
    #   a0 = hart id
    #   a1 = 栈顶虚拟地址（由BSP分配，并通过SBI的hart_start函数的opaque参数传递）
    
    # 设置 AP 栈指针 (从 a1 获取)
    mv      sp, a1

setup_paging:
    # 2. 启用分页（所有核心）
    # 设置第一级页表
    #   页表项 = (PPN(boot_pagetable_2nd) << 10) | 0x01 # 有效位V
    la     t1, boot_pagetable

    # 设置第256个页表项指向boot_pagetable_1st
    li     t0, 8 * 256
    add    t1, t1, t0
    la     t0, boot_pagetable_1st
    srli   t0, t0, 12
    slli   t0, t0, 10
    ori    t0, t0, 0x01
    sd     t0, 0(t1)

    # 设置第512个页表项指向boot_pagetable_2nd
    la     t1, boot_pagetable
    li     t0, 8 * 511
    add    t1, t1, t0
    la     t0, boot_pagetable_2nd
    srli   t0, t0, 12
    slli   t0, t0, 10
    ori    t0, t0, 0x01
    sd     t0, 0(t1)

    la     t0, boot_pagetable
    li     t1, 9 << 60
    srli   t0, t0, 12
    or     t0, t0, t1
    csrw   satp, t0
    sfence.vma

    # 3. 跳转到rust的riscv_boot，传递hart_id（a0）和dtb_paddr（a1，仅BSP有效）
    lga    t0, riscv_boot
    jr     t0


.section .bss.stack

.globl boot_stack_bottom
boot_stack_bottom:
    # 为 BSP 核心分配堆栈空间
    .space 0x40000

.globl boot_stack_top
boot_stack_top:


.section .data

.align 12
boot_pagetable:
    .quad (0x00000 << 10) | 0xcf # VRWXAD
    .zero 8 * 255
    .quad 0  # To-Be-Assign
    .zero 8 * 254
    .quad 0  # 指向第二个子页表 boot_pagetable_2nd

# 第一个子页表
boot_pagetable_1st:
    # 映射低地址空间
    .quad (0x00000 << 10) | 0xcf # VRWXAD
    .quad (0x40000 << 10) | 0xcf # VRWXAD
    .quad (0x80000 << 10) | 0xcf # VRWXAD
    .zero 8 * 509

# 第二个子页表
boot_pagetable_2nd:
    # 映射低地址空间
    .zero 8 * 508
    .quad (0x00000 << 10) | 0xcf # VRWXAD
    .quad (0x40000 << 10) | 0xcf # VRWXAD
    .quad (0x80000 << 10) | 0xcf # VRWXAD
    .quad 0

