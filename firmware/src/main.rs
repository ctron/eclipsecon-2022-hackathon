#![no_std]
#![no_main]
#![macro_use]
#![feature(generic_associated_types)]
#![feature(type_alias_impl_trait)]

use btmesh_device::{BluetoothMeshModel, BluetoothMeshModelContext};
use btmesh_macro::{device, element};
use btmesh_models::{
    generic::{
        battery::{GenericBatteryMessage, GenericBatteryServer},
        onoff::{GenericOnOffClient, GenericOnOffMessage, GenericOnOffServer},
    },
    sensor::{SensorMessage, SensorSetupMessage, SensorSetupServer, SensorStatus},
};
use btmesh_nrf_softdevice::*;
use core::future::Future;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
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
async fn main(_s: Spawner) {
    let board = Microbit::new(config());

    let mut driver = Driver::new("drogue", unsafe { &__storage as *const u8 as u32 }, 100);

    let sd = driver.softdevice();
    let sensor = Sensor::new(Duration::from_secs(5), sd);

    let mut device = Device::new(board.btn_a, board.btn_b, board.display, sensor);

    // Give flash some time before accessing
    Timer::after(Duration::from_millis(100)).await;

    driver.run(&mut device).await.unwrap();
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
    pub fn new(btn_a: Button, btn_b: Button, display: LedMatrix, sensor: Sensor) -> Self {
        Self {
            front: Front {
                display: DisplayOnOff::new(display),
                battery: Battery::new(),
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

struct ButtonOnOff {
    button: Button,
}

impl ButtonOnOff {
    fn new(button: Button) -> Self {
        Self { button }
    }
}

impl BluetoothMeshModel<GenericOnOffClient> for ButtonOnOff {
    type RunFuture<'f, C> = impl Future<Output=Result<(), ()>> + 'f
    where
        Self: 'f,
        C: BluetoothMeshModelContext<GenericOnOffClient> + 'f;

    fn run<'run, C: BluetoothMeshModelContext<GenericOnOffClient> + 'run>(
        &'run mut self,
        _: C,
    ) -> Self::RunFuture<'_, C> {
        async move {
            loop {
                self.button.wait_for_falling_edge().await;
                defmt::info!("** button pushed");
            }
        }
    }
}

struct DisplayOnOff {
    display: LedMatrix,
}

impl DisplayOnOff {
    fn new(display: LedMatrix) -> Self {
        Self { display }
    }
}

impl BluetoothMeshModel<GenericOnOffServer> for DisplayOnOff {
    type RunFuture<'f, C> = impl Future<Output=Result<(), ()>> + 'f
    where
        Self: 'f,
        C: BluetoothMeshModelContext<GenericOnOffServer> + 'f;

    fn run<'run, C: BluetoothMeshModelContext<GenericOnOffServer> + 'run>(
        &'run mut self,
        ctx: C,
    ) -> Self::RunFuture<'_, C> {
        async move {
            loop {
                let (message, _meta) = ctx.receive().await;
                match message {
                    GenericOnOffMessage::Get => {}
                    GenericOnOffMessage::Set(val) => {
                        if val.on_off != 0 {
                            self.display.scroll("ON").await;
                        }
                    }
                    GenericOnOffMessage::SetUnacknowledged(val) => {
                        if val.on_off != 0 {
                            self.display.scroll("OFF").await;
                        }
                    }
                    GenericOnOffMessage::Status(_) => {
                        // not applicable
                    }
                }
            }
        }
    }
}

struct Battery {}

impl Battery {
    pub fn new() -> Self {
        Self {}
    }
}

impl BluetoothMeshModel<GenericBatteryServer> for Battery {
    type RunFuture<'f, C> = impl Future<Output=Result<(), ()>> + 'f
    where
        Self: 'f,
        C: BluetoothMeshModelContext<GenericBatteryServer> + 'f;

    fn run<'run, C: BluetoothMeshModelContext<GenericBatteryServer> + 'run>(
        &'run mut self,
        ctx: C,
    ) -> Self::RunFuture<'_, C> {
        async move {
            loop {
                let (message, _meta) = ctx.receive().await;
                match message {
                    GenericBatteryMessage::Get => {}
                    GenericBatteryMessage::Status(_) => {}
                }
            }
        }
    }
}

type SensorServer = SensorSetupServer<MicrobitSensorConfig, 1, 1>;

pub struct Sensor {
    interval: Duration,
    sd: &'static Softdevice,
}

impl Sensor {
    pub fn new(interval: Duration, sd: &'static Softdevice) -> Self {
        Self { interval, sd }
    }

    async fn read(&mut self) -> Result<SensorPayload, ()> {
        let temperature: i8 = temperature_celsius(self.sd).map_err(|_| ())?.to_num();
        Ok(SensorPayload {
            temperature: temperature * 2,
        })
    }
}

impl BluetoothMeshModel<SensorServer> for Sensor {
    type RunFuture<'f, C> = impl Future<Output=Result<(), ()>> + 'f
    where
        Self: 'f,
        C: BluetoothMeshModelContext<SensorServer> + 'f;

    fn run<'run, C: BluetoothMeshModelContext<SensorServer> + 'run>(
        &'run mut self,
        ctx: C,
    ) -> Self::RunFuture<'_, C> {
        async move {
            loop {
                Timer::after(self.interval).await;

                match self.read().await {
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
                }
            }
        }
    }
}
