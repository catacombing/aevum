//! Rezz DBus interface.

use rezz::Alarm;
use zbus::proxy;

#[proxy(
    interface = "org.catacombing.rezz",
    default_service = "org.catacombing.rezz",
    default_path = "/org/catacombing/rezz"
)]
pub trait Rezz {
    async fn add_alarm(&self, alarm: Alarm) -> zbus::Result<()>;

    async fn remove_alarm(&self, id: String) -> zbus::Result<()>;

    #[zbus(property)]
    fn alarms(&self) -> zbus::Result<Vec<Alarm>>;
}
