# SPDX-License-Identifier: MPL-2.0

.global _start                      # 入口点
.section .text                      # 代码段
_start:
    li      a7, 64                  # 写入系统调用号
    li      a0, 1                   # 标准输出
    la      a1, message             # 消息地址
    la      a2, message_end         
    sub     a2, a2, a1              # 计算消息长度
    ecall                           # 系统调用
    li      a7, 93                  # 退出系统调用号
    li      a0, 0                   # 退出码
    ecall                           # 系统调用

.section .rodata                    # 只读数据段
message:
    .ascii  "Hello, world\n"
message_end:
