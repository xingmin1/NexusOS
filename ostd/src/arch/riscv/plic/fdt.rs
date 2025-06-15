use fdt::Fdt;

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
        .property_u32("riscv,ndev")
        .expect("PLIC riscv,ndev missing");

    /* interrupts-extended 由 (phandle,intid)* 组成，每对对应一个 context */
    let contexts_boxed = node
        .interrupts_extended()
        .map(|iter| iter.count() as u32 / 2)
        .map(|n| (0..n).collect::<alloc::vec::Vec<u32>>())
        .unwrap_or_default()
        .into_boxed_slice();

    PlicInfo {
        base,
        num_irqs,
        contexts: Box::leak(contexts_boxed),
    }
}
