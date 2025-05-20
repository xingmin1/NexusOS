use core::{
    fmt::{self, Write},
    sync::atomic::{AtomicUsize, Ordering},
};

use arrayvec::ArrayString; // 用于固定大小的栈上字符串
use crate::prelude::println;
use crate::trap;
use crate::{boot::boot_info, cpu_local_cell};
// For PER_CPU_INDENTATION
use tracing_core::{
    field::{Field, Visit},
    span::{Attributes, Id, Record},
    Event, Interest, Level, LevelFilter, Metadata, Subscriber,
};

/// 日志消息缓冲区的最大大小 (字节)。
/// 如果日志消息超出此大小，将会被截断。
const LOG_BUFFER_SIZE: usize = 1024;

/// 日志级别前缀
const LEVEL_TRACE_PREFIX: &str = "[TRACE]";
const LEVEL_DEBUG_PREFIX: &str = "[DEBUG]";
const LEVEL_INFO_PREFIX: &str = "[INFO]";
const LEVEL_WARN_PREFIX: &str = "[WARN]";
const LEVEL_ERROR_PREFIX: &str = "[ERROR]";

/// 用于生成唯一的 span ID (全局唯一)
static NEXT_SPAN_ID: AtomicUsize = AtomicUsize::new(1);

cpu_local_cell! {
    pub(crate) static PER_CPU_INDENTATION: usize = 0;
}
/// 每个缩进级别对应的空格数
const INDENT_MULTIPLIER: usize = 2;

/// 自定义的 Tracing 订阅器 (Subscriber)
/// 这是一个零大小类型 (ZST)，创建它没有运行时开销。
pub struct KernelTracer {
    max_level: LevelFilter,
}

impl KernelTracer {
    /// 创建一个新的内核追踪订阅器实例。
    ///
    /// # Arguments
    ///
    /// * `max_level` - 此订阅器将处理的最高日志级别。
    ///                 例如，如果设置为 `LevelFilter::INFO`，则 `INFO`, `WARN`, `ERROR` 级别的日志会被处理，
    ///                 而 `DEBUG` 和 `TRACE` 级别的日志将被忽略。
    pub fn new(max_level: LevelFilter) -> Self {
        KernelTracer { max_level }
    }

    /// 根据日志级别获取对应的前缀字符串。
    fn level_prefix(level: &Level) -> &'static str {
        match *level {
            Level::TRACE => LEVEL_TRACE_PREFIX,
            Level::DEBUG => LEVEL_DEBUG_PREFIX,
            Level::INFO => LEVEL_INFO_PREFIX,
            Level::WARN => LEVEL_WARN_PREFIX,
            Level::ERROR => LEVEL_ERROR_PREFIX,
        }
    }

    /// 安全地读取当前 CPU 的缩进级别。
    /// `PER_CPU_INDENTATION.load()` 内部会处理必要的同步（如禁用中断）。
    #[inline(always)]
    fn get_current_cpu_indent_level() -> usize {
        PER_CPU_INDENTATION.load()
    }
}

/// 实现字段访问 (Visit)，用于格式化追踪事件和 Span 中的字段。
/// 直接写入传入的 `ArrayString` 缓冲区。
struct FieldVisitor<'a> {
    buffer: &'a mut ArrayString<LOG_BUFFER_SIZE>,
    is_empty: bool, // 标记缓冲区当前是否为空，用于决定是否添加字段分隔符
}

impl<'a> FieldVisitor<'a> {
    fn new(buffer: &'a mut ArrayString<LOG_BUFFER_SIZE>) -> Self {
        buffer.clear(); // 确保从一个干净的缓冲区开始
        Self {
            buffer,
            is_empty: true,
        }
    }

    /// 如果缓冲区非空，则添加字段分隔符 ", "。
    fn append_separator(&mut self) {
        if !self.is_empty {
            // 忽略 ArrayString 可能因容量不足而产生的写入错误，消息会被截断。
            let _ = self.buffer.try_push_str(", ");
        }
    }

    /// 记录一个实现了 `fmt::Display` 的值。
    fn record_value<V: fmt::Display>(&mut self, value: V) {
        // 忽略写入错误，允许消息截断。
        let _ = write!(self.buffer, "{}", value);
        self.is_empty = false;
    }

    /// 记录一个实现了 `fmt::Debug` 的值。
    fn record_debug_value<V: fmt::Debug>(&mut self, value: V) {
        let _ = write!(self.buffer, "{:?}", value);
        self.is_empty = false;
    }

    /// 记录一个命名的、实现了 `fmt::Display` 的字段。
    fn record_named_value<V: fmt::Display>(&mut self, name: &str, value: V) {
        self.append_separator();
        let _ = write!(self.buffer, "{}={}", name, value);
        self.is_empty = false;
    }

    /// 记录一个命名的、实现了 `fmt::Debug` 的字段。
    fn record_named_debug_value<V: fmt::Debug>(&mut self, name: &str, value: V) {
        self.append_separator();
        let _ = write!(self.buffer, "{}={:?}", name, value);
        self.is_empty = false;
    }
}

impl<'a> Visit for FieldVisitor<'a> {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        let name = field.name();
        // "message" 字段通常是事件的主要可读信息，优先显示。
        if name == "message" && self.is_empty {
            self.record_debug_value(value);
        } else {
            self.record_named_debug_value(name, value);
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        let name = field.name();
        if name == "message" && self.is_empty {
            self.record_value(value);
        } else {
            // 非 "message" 的字符串字段用引号括起来，以示区分。
            self.append_separator();
            let _ = write!(self.buffer, "{}=\"{}\"", name, value);
            self.is_empty = false;
        }
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.record_named_value(field.name(), value);
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.record_named_value(field.name(), value);
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.record_named_value(field.name(), value);
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.record_named_value(field.name(), value);
    }
}

impl Subscriber for KernelTracer {
    /// 检查是否应启用具有给定元数据的追踪点。
    /// 此处简单地启用所有追踪。可以根据 `metadata.level()` 或 `metadata.target()` 实现过滤。
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        // 根据初始化时设置的 `max_level` 和追踪点的实际级别进行过滤。
        // `Level` 和 `LevelFilter` 的 `PartialOrd` 实现使得更"不重要"的级别（如 TRACE, DEBUG）具有更大的数值。
        // 如果一个事件的级别（数值）小于或等于过滤器的级别（数值），那么它就应该被启用。
        // 例如，如果过滤器是 `INFO` (数值3)，那么 `ERROR` (1), `WARN` (2), `INFO` (3) 都满足 `level_value <= 3`，所以它们被启用。
        // 而 `DEBUG` (4) 和 `TRACE` (5) 不满足，所以它们被禁用。
        metadata.level() <= &self.max_level
    }

    /// 当一个新的 Span 被创建时调用。
    fn new_span(&self, attrs: &Attributes<'_>) -> Id {
        // 为 Span 生成一个全局唯一的 ID。
        let id_val = NEXT_SPAN_ID.fetch_add(1, Ordering::Relaxed);
        let id = Id::from_u64(id_val as u64);
        let metadata = attrs.metadata();

        // 使用栈上的 ArrayString 来格式化字段，避免堆分配。
        let mut buffer = ArrayString::<LOG_BUFFER_SIZE>::new();
        {
            let mut field_visitor = FieldVisitor::new(&mut buffer);
            attrs.record(&mut field_visitor); // 记录 Span 的字段
        }

        // 获取当前 CPU 的缩进级别用于打印。
        let indent_val = Self::get_current_cpu_indent_level() * INDENT_MULTIPLIER;

        // 打印 Span 进入信息。
        // 注意: 如果字段过多导致 buffer 溢出，buffer.as_str() 将包含截断后的内容。
        println!(
            "{} [{}] {:indent$}--> {}[{}] {}",
            Self::level_prefix(metadata.level()), // 日志级别
            metadata.target(),                    // 目标模块
            "",                                   // 用于缩进的空字符串占位符
            metadata.name(),                      // Span 名称
            id.into_u64(),                        // Span ID
            buffer.as_str(),                      // 格式化后的字段
            indent = indent_val                   // 缩进量
        );
        id // 返回新创建的 Span ID
    }

    /// 当 Span 中的字段被记录时调用。
    fn record(&self, span: &Id, values: &Record<'_>) {
        let mut buffer = ArrayString::<LOG_BUFFER_SIZE>::new();
        {
            let mut field_visitor = FieldVisitor::new(&mut buffer);
            values.record(&mut field_visitor);
        }

        let indent_val = Self::get_current_cpu_indent_level() * INDENT_MULTIPLIER;
        println!(
            "{} {:indent$}... Span[{}] recorded: {}",
            LEVEL_INFO_PREFIX, // 默认为 INFO 级别，或可从存储的 Span 数据中获取
            "",
            span.into_u64(),
            buffer.as_str(),
            indent = indent_val
        );
    }

    /// 当一个 Span 表明它跟随另一个 Span 时调用。
    fn record_follows_from(&self, span: &Id, follows: &Id) {
        let indent_val = Self::get_current_cpu_indent_level() * INDENT_MULTIPLIER;
        println!(
            "{} {:indent$}Span[{}] follows from Span[{}]",
            LEVEL_INFO_PREFIX, // 默认为 INFO 级别
            "",
            span.into_u64(),
            follows.into_u64(),
            indent = indent_val
        );
    }

    /// 当一个追踪事件发生时调用。
    fn event(&self, event: &Event<'_>) {
        let metadata = event.metadata();
        let mut buffer = ArrayString::<LOG_BUFFER_SIZE>::new();
        {
            let mut field_visitor = FieldVisitor::new(&mut buffer);
            event.record(&mut field_visitor); // 记录事件的字段
        }

        let indent_val = Self::get_current_cpu_indent_level() * INDENT_MULTIPLIER;

        println!(
            "{} [{}] {:indent$}{}",
            Self::level_prefix(metadata.level()),
            metadata.target(),
            "",
            buffer.as_str(),
            indent = indent_val
        );
    }

    /// 当执行进入一个 Span 时调用。
    fn enter(&self, _span: &Id) {
        // 为了确保在中断处理程序中也能安全地修改 PER_CPU_INDENTATION (如果中断可以追踪),
        // 整个读-改-写序列需要在禁用本地中断的情况下原子地执行。
        // `PER_CPU_INDENTATION.load()` 和 `store()` 各自内部会禁用中断以保证单次操作的原子性，
        // 但整个序列的原子性需要外部的 `trap::disable_local()` 守卫。
        let _guard = trap::disable_local(); // 禁用当前CPU的本地中断

        let current_val = PER_CPU_INDENTATION.load();
        PER_CPU_INDENTATION.store(current_val.saturating_add(1)); // 增加缩进级别
    }

    /// 当执行退出一个 Span 时调用。
    fn exit(&self, span: &Id) {
        let new_indent_level;
        {
            // 同样，为了保证读-改-写序列的原子性，禁用本地中断。
            let _guard = trap::disable_local(); // 禁用当前CPU的本地中断

            let current_val = PER_CPU_INDENTATION.load();
            new_indent_level = current_val.saturating_sub(1); // 减少缩进级别
            PER_CPU_INDENTATION.store(new_indent_level);
        } // _guard 在作用域结束时自动恢复中断先前的状态

        // 退出日志的缩进级别应为减少后的级别 (即父 Span 的缩进级别)。
        let indent_val_for_print = new_indent_level * INDENT_MULTIPLIER;

        println!(
            "{} {:indent$}<-- Span exited (ID: {})",
            LEVEL_INFO_PREFIX, // 默认为 INFO 级别
            "",
            span.into_u64(),
            indent = indent_val_for_print
        );
    }

    /// 注册一个调用点，并返回其 `Interest`。
    /// `Interest::always()` 表示对所有由此调用点产生的事件都感兴趣。
    fn register_callsite(&self, metadata: &'static Metadata<'static>) -> Interest {
        if self.enabled(metadata) {
            Interest::always()
        } else {
            Interest::never()
        }
    }

    /// 克隆一个 Span ID。
    fn clone_span(&self, id: &Id) -> Id {
        id.clone()
    }

    /// 尝试关闭一个 Span。
    /// 对于此简单实现，可以认为 Span 在退出时即关闭。
    fn try_close(&self, _id: Id) -> bool {
        true
    }
}

/// 辅助函数：将字符串形式的日志级别转换为 `LevelFilter`。
///
/// # Arguments
///
/// * `level_str` - 代表日志级别的字符串，不区分大小写 (例如 "trace", "INFO", "WaRn")。
///
/// # Returns
///
/// 对应的 `LevelFilter`。如果字符串无法识别，则打印警告并返回 `LevelFilter::INFO`。
fn parse_log_level(level_str: &str) -> LevelFilter {
    match level_str.to_lowercase().as_str() {
        "trace" => LevelFilter::TRACE,
        "debug" => LevelFilter::DEBUG,
        "info" => LevelFilter::INFO,
        "warn" => LevelFilter::WARN,
        "error" => LevelFilter::ERROR,
        "off" => LevelFilter::OFF,
        unknown => {
            println!(
                "[TRACING] Unknown log level string: '{}'. Defaulting to INFO.",
                unknown
            );
            LevelFilter::INFO // 默认级别
        }
    }
}

/// 初始化并设置全局默认的追踪订阅器。
///
/// **重要**: 此函数应在 `ostd::cpu::local` 子系统 (包括 `CpuLocalCell` 的支持)
/// 完全初始化之后调用。`PER_CPU_INDENTATION` 依赖于 CPU 本地存储的正确设置。
pub fn init_tracing() {
    let kcmdline = &boot_info().kernel_cmdline;

    let value = kcmdline
        .split(' ')
        .find(|arg| arg.starts_with("ostd.log_level="))
        .map(|arg| arg.split('=').next_back().unwrap_or_default());

    let log_level_str = value.unwrap_or("info");
    let max_level_filter = parse_log_level(log_level_str);

    let subscriber = KernelTracer::new(max_level_filter);
    match tracing::subscriber::set_global_default(subscriber) {
        Ok(_) => tracing::info!(
            "KernelTracer initialized (max_level: {}). Per-CPU indent, no_alloc, no_unsafe in tracer.",
            max_level_filter
        ),
        Err(e) => println!(
            "[TRACING] Failed to set global subscriber: {:?}. Already set?",
            e
        ),
    }
}
