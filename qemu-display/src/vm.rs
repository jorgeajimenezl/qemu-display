#[zbus::proxy(
    default_service = "org.qemu",
    interface = "org.qemu.Display1.VM",
    default_path = "/org/qemu/Display1/VM"
)]
pub trait VM {
    /// Name property
    #[zbus(property)]
    fn name(&self) -> zbus::Result<String>;

    /// UUID property
    #[zbus(property, name = "UUID")]
    fn uuid(&self) -> zbus::Result<String>;

    /// ConsoleIDs property
    #[zbus(property, name = "ConsoleIDs")]
    fn console_ids(&self) -> zbus::Result<Vec<u32>>;

    /// Interfaces property
    #[zbus(property)]
    fn interfaces(&self) -> zbus::Result<Vec<String>>;
}
