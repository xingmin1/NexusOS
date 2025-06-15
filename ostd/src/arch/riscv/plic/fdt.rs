use alloc::boxed::Box;
pub use fdt::Fdt;

/// FDT 提取到的关键信息
pub struct PlicInfo {
    pub base: usize,          // 物理基址
    pub num_irqs: u32,        // 可用 IRQ 数
    pub contexts: &'static [u32], // hart_id -> context_id（S mode）
}

/// 解析节点，兼容多厂商 SoC
pub fn parse(fdt: &Fdt<'_>) -> PlicInfo {
    let node = fdt
        .find_compatible(&["sifive,plic-1.0.0", "riscv,plic0"])
        .expect("PLIC node not found in DTB");

    /* reg 属性：<base size> */
    let region = node.reg().expect("PLIC reg missing").next().unwrap();
    let base   = region.starting_address as usize;

    /* riscv,ndev 指示外部 IRQ 数量 */
    let num_irqs = node
        .property("riscv,ndev")
        .map(|p| p.as_usize().expect("PLIC riscv,ndev missing"))
        .expect("PLIC riscv,ndev missing");

    /* interrupts-extended 由若干 (phandle, intid) 二元组组成，
     * 其中每个元素（cell）为 32bit。直接按 u32 切片解析即可。 */
    let contexts_boxed = node
        .property("interrupts-extended")
        .map(|prop| {
            use core::convert::TryInto;

            let bytes = prop.value;
            // 按 32bit 大端 cell 切片
            let mut ctxs = alloc::vec::Vec::new();

            // 每个 context 占 2 个 cell：(phandle, intid)
            for pair in bytes.chunks_exact(8) {
                // 取第二个 cell 作为 context id
                let ctx = u32::from_be_bytes(pair[4..8].try_into().unwrap());
                // 过滤 0xFFFF_FFFF (占位/无效) 且仅保留奇数 (S-mode) context
                if ctx != u32::MAX && (ctx & 1) == 1 {
                    ctxs.push(ctx);
                }
            }

            ctxs
        })
        .unwrap_or_default()
        .into_boxed_slice();

    PlicInfo {
        base,
        num_irqs: num_irqs as u32,
        contexts: Box::leak(contexts_boxed),
    }
}
