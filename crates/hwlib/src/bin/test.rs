use hwlib::sensors::SensorHub;

#[tokio::main]
async fn main() {
    let mut sensor_hub = SensorHub::new();
    hwlib::driver::init_driver().unwrap();
    sensor_hub.set_pawn_manager(
        hwlib::driver::get_driver()
            .unwrap()
            .pawn_manager()
            .unwrap()
            .clone(),
    );
    match sensor_hub.detect() {
        Ok(_) => println!("硬件检测成功"),
        Err(e) => eprintln!("硬件检测失败: {:?}", e),
    }

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        match sensor_hub.read_all() {
            Ok((cpu_sensors, mb_sensors)) => {
                println!("CPU 传感器数据:");
                for sensor in cpu_sensors {
                    println!("{:?}", sensor);
                }

                println!("主板传感器数据:");
                for sensor in mb_sensors {
                    println!("{:?}", sensor);
                }
            }
            Err(e) => eprintln!("读取传感器数据失败: {:?}", e),
        }
    }
}
