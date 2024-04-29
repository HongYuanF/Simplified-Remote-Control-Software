use crate::key_mouse;
use crate::screen::Cap;
use enigo::Enigo;
use enigo::KeyboardControllable;
use enigo::MouseControllable;
use flate2::write::DeflateEncoder;
use flate2::Compression;
use rayon::prelude::*;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;
use std::io::Read;
use std::io::Write;
use std::net::TcpListener;
use std::net::TcpStream;
use std::sync::mpsc::channel;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::Sender;

pub struct Server {
    port: u16,    // 默认端口为80
    pwd: [u8; 8], // 存储密码的哈希值
}

impl Server {
    // 创建一个新的 Server 实例
    pub fn new(port: u16, pwd: String) -> Self {
        // 直接计算密码哈希并存储
        let mut hasher = DefaultHasher::new();
        hasher.write(pwd.as_bytes());
        let pk = hasher.finish();
        let pwd = pk.to_be_bytes();

        Self { port, pwd }
    }

    // 处理密码验证和处理连接的主要函数
    pub fn run(&self) {
        // 启动 TCP 监听器并获取用于从中接收 TCP 流的接收器
        let rx = self.run_tcp_listeners();

        // 循环接收 TCP 流并处理
        loop {
            match rx.recv() {
                Ok(mut stream) => {
                    // 检查密码是否正确
                    if let Err(_) = self.check_pwd(&mut stream) {
                        continue;
                    }

                    // 发送成功信号给客户端
                    if let Err(_) = stream.write_all(&[1]) {
                        continue;
                    }

                    // 克隆一个 TCP 流以用于不同的线程
                    let ss = stream.try_clone().unwrap();

                    // 创建两个线程，一个用于处理屏幕流，另一个用于接收和播放事件
                    let th1 = std::thread::spawn(move || {
                        if let Err(e) = std::panic::catch_unwind(|| {
                            screen_stream(ss);
                        }) {
                            eprintln!("{:?}", e);
                        }
                    });

                    let th2 = std::thread::spawn(move || {
                        if let Err(e) = std::panic::catch_unwind(|| {
                            recv_and_play_events(stream);
                        }) {
                            eprintln!("{:?}", e);
                        }
                    });

                    // 等待两个线程结束
                    th1.join().unwrap();
                    th2.join().unwrap();
                    println!("Break !");
                }
                // 接收器错误，停止运行
                Err(_) => {
                    return;
                }
            }
        }
    }

    // 启动 TCP 监听器并返回用于接收 TCP 流的接收器
    fn run_tcp_listeners(&self) -> Receiver<TcpStream> {
        let (tx6, rx) = channel::<TcpStream>();
        let _port = self.port;

        // 启动两个监听线程，IPv4 和 IPv6
        let _ = std::thread::spawn(move || {
            if cfg!(target_os = "windows") {
                let tx4 = tx6.clone();
                Self::start_tcp_listener(&format!("0.0.0.0:{}", _port), tx4);
            }
            Self::start_tcp_listener(&format!("[::0]:{}", _port), tx6);
        });

        rx
    }

    // TCP 监听器函数
    fn start_tcp_listener(bind_address: &str, tx: Sender<TcpStream>) {
        let listener = TcpListener::bind(bind_address).unwrap();
        println!("Listening on {}", bind_address);

        for stream in listener.incoming() {
            match stream {
                // 将接收到的流发送到通道
                Ok(stream) => {
                    if tx.send(stream).is_err() {
                        eprintln!("Failed to send the stream through the channel");
                        break;
                    }
                }
                // 处理连接错误
                Err(e) => eprintln!("Connection failed: {}", e),
            }
        }
    }

    // 检查密码是否正确
    fn check_pwd(&self, stream: &mut TcpStream) -> Result<(), ()> {
        let mut check = [0u8; 8];

        if let Ok(_) = stream.read_exact(&mut check) {
            // 检查密码与哈希是否匹配
            if check != self.pwd {
                println!("Password error");
                let _ = stream.write_all(&[2]); // 发送错误消息给客户端
                return Err(());
            }
        } else {
            println!("Request error");
            return Err(());
        }

        Ok(())
    }
}

/// 从接收的信息，来模拟client的键鼠移动
fn recv_and_play_events(mut stream: TcpStream) {
    let mut cmd = [0u8];
    let mut move_cmd = [0u8; 4];
    let mut enigo = Enigo::new();
    while let Ok(_) = stream.read_exact(&mut cmd) {
        match cmd[0] {
            communication::KEY_UP => {
                stream.read_exact(&mut cmd).unwrap();
                if let Some(key) = key_mouse::key_to_enigo(cmd[0]) {
                    enigo.key_up(key);
                }
            }
            communication::KEY_DOWN => {
                stream.read_exact(&mut cmd).unwrap();
                if let Some(key) = key_mouse::key_to_enigo(cmd[0]) {
                    enigo.key_down(key);
                }
            }
            communication::MOUSE_KEY_UP => {
                stream.read_exact(&mut cmd).unwrap();
                if let Some(key) = key_mouse::mouse_to_engin(cmd[0]) {
                    enigo.mouse_up(key);
                }
            }
            communication::MOUSE_KEY_DOWN => {
                stream.read_exact(&mut cmd).unwrap();
                if let Some(key) = key_mouse::mouse_to_engin(cmd[0]) {
                    enigo.mouse_down(key);
                }
            }
            communication::MOUSE_WHEEL_UP => {
                enigo.mouse_scroll_y(-2);
            }
            communication::MOUSE_WHEEL_DOWN => {
                enigo.mouse_scroll_y(2);
            }
            communication::MOVE => {
                stream.read_exact(&mut move_cmd).unwrap();
                let x = ((move_cmd[0] as i32) << 8) | (move_cmd[1] as i32);
                let y = ((move_cmd[2] as i32) << 8) | (move_cmd[3] as i32);
                enigo.mouse_move_to(x, y);
            }
            _ => {
                return;
            }
        }
    }
}

/**
 * 编码数据header
 */
#[inline]
fn encode(data_len: usize, res: &mut [u8]) {
    res[0] = (data_len >> 16) as u8;
    res[1] = (data_len >> 8) as u8;
    res[2] = data_len as u8;
}

/*
图像字节序
+------------+
|     24     |
+------------+
|   length   |
+------------+
|   data     |
+------------+
length: 数据长度
data: 数据
*/
fn screen_stream(mut stream: TcpStream) {
    let mut cap = Cap::new();

    let (w, h) = cap.wh();

    // 发送w, h
    let mut meta = [0u8; 4];
    meta[0] = (w >> 8) as u8;
    meta[1] = w as u8;
    meta[2] = (h >> 8) as u8;
    meta[3] = h as u8;
    if let Err(_) = stream.write_all(&meta) {
        return;
    }
    let mut header = [0u8; 3];
    let mut yuv = Vec::<u8>::new();
    let mut last = Vec::<u8>::new();
    // 第一帧
    let bgra = cap.cap();
    communication::convert::bgra_to_i420(w, h, bgra, &mut yuv);
    // 压缩
    let mut buf = Vec::<u8>::with_capacity(1024 * 4);
    let mut e = DeflateEncoder::new(buf, Compression::default());
    e.write_all(&yuv).unwrap();
    buf = e.reset(Vec::new()).unwrap();
    (last, yuv) = (yuv, last);

    let clen = buf.len();
    encode(clen, &mut header);
    if let Err(_) = stream.write_all(&header) {
        return;
    }
    if let Err(_) = stream.write_all(&buf) {
        return;
    }
    loop {
        let bgra = cap.cap();
        unsafe {
            yuv.set_len(0);
        }
        communication::convert::bgra_to_i420(w, h, bgra, &mut yuv);
        if yuv[..w * h] == last[..w * h] {
            continue;
        }
        last.par_iter_mut().zip(yuv.par_iter()).for_each(|(a, b)| {
            *a = *a ^ *b;
        });
        // 压缩
        unsafe {
            buf.set_len(0);
        }
        e.write_all(&last).unwrap();
        buf = e.reset(buf).unwrap();
        (last, yuv) = (yuv, last);
        // 发送
        let clen = buf.len();
        encode(clen, &mut header);
        if let Err(_) = stream.write_all(&header) {
            return;
        }
        if let Err(_) = stream.write_all(&buf) {
            return;
        }
    }
}
