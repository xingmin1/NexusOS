
use ::fdt::Fdt;

/// 解析结果
pub struct Info {
    pub platic_base: usize,
    pub core_cnt:    usize,
}

/// - `loongson,pch-pic-1.0` 节点：PLATIC MMIO  
/// - `/cpus/cpu@*`          ：核数
pub fn parse(dt: &Fdt) -> Info {
    let platic = dt
        .find_compatible(&["loongson,pch-pic-1.0"])
        .expect("[PLIC] DTB 缺少 platic 节点");

    let reg = platic
        .reg()
        .and_then(|mut r| r.next())
        .expect("[PLIC] platic 节点无 reg");

    Info {
        platic_base: reg.starting_address as usize,
        core_cnt: dt.find_all_nodes("/cpus/cpu").count(),
    }
}
