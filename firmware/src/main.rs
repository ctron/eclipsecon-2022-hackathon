#![no_std]
#![no_main]
#![macro_use]
#![feature(generic_associated_types)]
#![feature(type_alias_impl_trait)]

mod battery;
mod button;
mod display;

use battery::*;
use btmesh_device::{
    BluetoothMeshModel, BluetoothMeshModelContext, Control, InboundModelPayload, PublicationCadence,
};
use btmesh_macro::{device, element};
use btmesh_models::sensor::{SensorMessage, SensorSetupMessage, SensorSetupServer, SensorStatus};
use btmesh_nrf_softdevice::*;
use button::*;
use core::future::Future;
use display::*;
use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_time::{Duration, Ticker, Timer};
use futures::StreamExt;
use microbit_async::*;
use nrf_softdevice::{temperature_celsius, Softdevice};
use sensor_model::*;

extern "C" {
    static __storage: u8;
}

use defmt_rtt as _;
use panic_probe as _;

// Application must run at a lower priority than softdevice
fn config() -> Config {
    let mut config = embassy_nrf::config::Config::default();
    config.gpiote_interrupt_priority = Priority::P2;
    config.time_interrupt_priority = Priority::P2;
    config
}

#[embassy_executor::main]
async fn main(s: Spawner) {
    let board = Microbit::new(config());

    let driver = Driver::new(
        "drogue",
        unsafe { &__storage as *const u8 as u32 },
        100,
        None,
    );

    let sd = driver.softdevice();
    let sensor = Sensor::new(sd);
    let battery = Battery::new();
    let display = DisplayOnOff::new(board.display);

    let device = Device::new(board.btn_a, board.btn_b, display, battery, sensor);

    // Give flash some time before accessing
    Timer::after(Duration::from_millis(100)).await;

    s.spawn(driver_task(driver, device)).unwrap();
}

#[embassy_executor::task]
async fn driver_task(mut driver: Driver, mut device: Device) {
    loop {
        let _ = driver.run(&mut device).await;
    }
}

#[device(cid = 0x0003, pid = 0x0001, vid = 0x0001)]
pub struct Device {
    front: Front,
    btn_a: ButtonA,
    btn_b: ButtonB,
}

#[element(location = "front")]
struct Front {
    display: DisplayOnOff,
    battery: Battery,
    sensor: Sensor,
}

#[element(location = "left")]
struct ButtonA {
    button: ButtonOnOff,
}

#[element(location = "right")]
struct ButtonB {
    button: ButtonOnOff,
}

impl Device {
    pub fn new(
        btn_a: Button,
        btn_b: Button,
        display: DisplayOnOff,
        battery: Battery,
        sensor: Sensor,
    ) -> Self {
        Self {
            front: Front {
                display,
                battery,
                sensor,
            },
            btn_a: ButtonA {
                button: ButtonOnOff::new(btn_a),
            },
            btn_b: ButtonB {
                button: ButtonOnOff::new(btn_b),
            },
        }
    }
}

type SensorServer = SensorSetupServer<MicrobitSensorConfig, 1, 1>;

pub struct Sensor {
    sd: &'static Softdevice,
    ticker: Option<Ticker>,
}

impl Sensor {
    pub fn new(sd: &'static Softdevice) -> Self {
        Self { sd, ticker: None }
    }

    async fn read(&mut self) -> Result<SensorPayload, ()> {
        let temperature: i8 = temperature_celsius(self.sd).map_err(|_| ())?.to_num();
        Ok(SensorPayload {
            temperature: temperature * 2,
        })
    }

    async fn process<C: BluetoothMeshModelContext<SensorServer>>(
        &mut self,
        _ctx: &mut C,
        data: &InboundModelPayload<SensorSetupMessage<MicrobitSensorConfig, 1, 1>>,
    ) {
        match data {
            InboundModelPayload::Control(Control::PublicationCadence(cadence)) => match cadence {
                PublicationCadence::Periodic(cadence) => {
                    defmt::info!("Enabling sensor publish at {:?}", cadence.as_secs());
                    self.ticker.replace(Ticker::every(*cadence));
                }
                PublicationCadence::OnChange => {
                    defmt::info!("Sensor publish on change!");
                    self.ticker.take();
                }
                PublicationCadence::None => {
                    defmt::info!("Disabling sensor publish");
                    self.ticker.take();
                }
            },
            _ => {}
        }
    }
}

impl BluetoothMeshModel<SensorServer> for Sensor {
    type RunFuture<'f, C> = impl Future<Output=Result<(), ()>> + 'f
    where
        Self: 'f,
        C: BluetoothMeshModelContext<SensorServer> + 'f;

    fn run<'run, C: BluetoothMeshModelContext<SensorServer> + 'run>(
        &'run mut self,
        mut ctx: C,
    ) -> Self::RunFuture<'_, C> {
        async move {
            loop {
                if let Some(ticker) = self.ticker.as_mut() {
                    match select(ctx.receive(), ticker.next()).await {
                        Either::First(data) => self.process(&mut ctx, &data).await,
                        Either::Second(_) => match self.read().await {
                            Ok(result) => {
                                defmt::info!("Read sensor data: {:?}", result);
                                let message = SensorSetupMessage::Sensor(SensorMessage::Status(
                                    SensorStatus::new(result),
                                ));
                                match ctx.publish(message).await {
                                    Ok(_) => {
                                        defmt::info!("Published sensor reading");
                                    }
                                    Err(e) => {
                                        defmt::warn!("Error publishing sensor reading: {:?}", e);
                                    }
                                }
                            }
                            Err(e) => {
                                defmt::warn!("Error reading sensor data: {:?}", e);
                            }
                        },
                    }
                } else {
                    let m = ctx.receive().await;
                    self.process(&mut ctx, &m).await;
                }
            }
        }
    }
}
