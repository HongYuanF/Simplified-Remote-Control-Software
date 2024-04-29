use flate2::write::DeflateDecoder;
use fltk::button::Button;
use fltk::draw;
use fltk::enums::Color;
use fltk::frame::Frame;
use fltk::input::Input;
use fltk::input::SecretInput;
use fltk::prelude::InputExt;
use fltk::window::Window;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;
use std::io::Read;
use std::io::Result;
use std::io::Write;
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::RwLock;

use fltk::app;
use fltk::enums;
use fltk::enums::Event;
use fltk::image;
use fltk::prelude::GroupExt;
use fltk::prelude::ImageExt;
use fltk::prelude::WidgetBase;
use fltk::prelude::WidgetExt;
use rayon::prelude::*;

use crate::bitmap;

/// client的主控制函数，绘制窗口
pub fn run() {
    let app = app::App::default();
    let (sw, sh) = app::screen_size();
    // 开始绘制wind窗口
    let mut wind = Window::new(
        (sw / 2.0) as i32 - 170,
        (sh / 2.0) as i32 - 70,
        340,
        140,
        "Diffscreen",
    );
    wind.set_color(Color::from_rgb(255, 255, 255));
    let mut host_ipt = Input::new(80, 20, 200, 25, "HOST:");
    host_ipt.set_value("127.0.0.1:80");
    let mut pwd_ipt = SecretInput::new(80, 50, 200, 25, "PASS:");
    pwd_ipt.set_value("diffscreen");
    let mut login_btn = Button::new(200, 80, 80, 40, "Login");
    // wind窗口结束绘制
    wind.end();
    wind.show();

    // 按下登陆键
    login_btn.set_callback(move |_| {
        wind.hide();
        log_in_and_run(host_ipt.value(), pwd_ipt.value());
    });
    app.run().unwrap();
}

enum Msg {
    Draw,
}

// 解包
#[inline]
fn depack(buffer: &[u8]) -> usize {
    ((buffer[0] as usize) << 16) | ((buffer[1] as usize) << 8) | (buffer[2] as usize)
}

/// 运行客户端
fn log_in_and_run(host: String, pwd: String) {
    // 与服务器建立链接
    let mut conn = TcpStream::connect(host).unwrap();
    let _ = validate_password(&mut conn, &pwd);

    // 开始绘制wind2窗口
    let (sw, sh) = app::screen_size();
    let mut wind_screen = Window::default()
        .with_size((sw / 2.0) as i32, (sh / 2.0) as i32)
        .with_label("简易版远程控制");
    let mut frame = Frame::default().size_of(&wind_screen);
    wind_screen.make_resizable(true);
    wind_screen.end();
    wind_screen.show();

    // 接收meta信息
    let mut meta = [0u8; 4];
    if let Err(_) = conn.read_exact(&mut meta) {
        return;
    }
    let w = (((meta[0] as u16) << 8) | meta[1] as u16) as i32;
    let h = (((meta[2] as u16) << 8) | meta[3] as u16) as i32;

    let dlen = (w * h * 3) as usize;

    let work_buf = Arc::new(RwLock::new(vec![0u8; dlen]));
    let draw_work_buf = work_buf.clone();

    let conn_clone = conn.try_clone().unwrap();
    deal_with_events(w, h, &mut frame, conn_clone);

    let _tool_str = Arc::new(RwLock::new(String::new()));
    let _tool_strc = _tool_str.clone();

    // 重画
    frame.draw(move |frame| {
        if let Ok(_buf) = draw_work_buf.read() {
            unsafe {
                if let Ok(mut image) =
                    image::RgbImage::from_data2(&_buf, w, h, enums::ColorDepth::Rgb8 as i32, 0)
                {
                    image.scale(frame.width(), frame.height(), false, true);
                    image.draw(frame.x(), frame.y(), frame.width(), frame.height());
                    draw::set_color_rgb(0, 0, 0);
                    if let Ok(a) = _tool_strc.read() {
                        draw::draw_text(&a, frame.x() + frame.width() - 180, 20);
                    }
                }
            }
        }
    });

    let (tx, rx) = app::channel::<Msg>();

    // 用来接收图像信息，并通知主线程重画
    std::thread::spawn(move || {
        let u = (w * h) as usize;
        let v = u + u / 4;
        let mut yuv = Vec::<u8>::new();
        let mut _yuv = Vec::<u8>::new();
        let mut buf = Vec::<u8>::new();

        // FPS
        let mut last = std::time::Instant::now();
        let mut fps = 0u8;
        let mut fpscount = 0u8;
        // 流速
        let mut _length_all = 0usize;
        let mut _length_sum = 0usize;
        // 接收第一帧数据
        let mut header = [0u8; 3];
        if let Err(_) = conn.read_exact(&mut header) {
            return;
        }
        let recv_len = depack(&header);
        _length_sum += recv_len;

        if buf.capacity() < recv_len {
            buf.resize(recv_len, 0u8);
        }
        if let Err(e) = conn.read_exact(&mut buf) {
            println!("error {}", e);
            return;
        }
        unsafe {
            yuv.set_len(0);
        }
        let mut d = DeflateDecoder::new(yuv);
        d.write_all(&buf).unwrap();
        yuv = d.reset(Vec::new()).unwrap();

        if let Ok(mut _buf) = work_buf.write() {
            communication::convert::i420_to_rgb(
                w as usize,
                h as usize,
                &yuv[..u],
                &yuv[u..v],
                &yuv[v..],
                &mut _buf,
            );
        }
        (_yuv, yuv) = (yuv, _yuv);
        tx.send(Msg::Draw);

        loop {
            if let Err(_) = conn.read_exact(&mut header) {
                return;
            }
            let recv_len = depack(&header);
            _length_sum += recv_len;

            if buf.capacity() < recv_len {
                buf.resize(recv_len, 0u8);
            } else {
                unsafe {
                    buf.set_len(recv_len);
                }
            }
            if let Err(_) = conn.read_exact(&mut buf) {
                return;
            }
            unsafe {
                yuv.set_len(0);
            }
            d.write_all(&buf).unwrap();
            yuv = d.reset(yuv).unwrap();

            yuv.par_iter_mut().zip(_yuv.par_iter()).for_each(|(a, b)| {
                *a = *b ^ *a;
            });

            if let Ok(mut _buf) = work_buf.write() {
                communication::convert::i420_to_rgb(
                    w as usize,
                    h as usize,
                    &yuv[..u],
                    &yuv[u..v],
                    &yuv[v..],
                    &mut _buf,
                );
            }
            (_yuv, yuv) = (yuv, _yuv);
            {
                let cur = std::time::Instant::now();
                let dur = cur.duration_since(last);
                fpscount += 1;
                if dur.as_millis() >= 1000 {
                    last = cur;
                    _length_all = _length_sum;
                    if let Ok(mut a) = _tool_str.write() {
                        *a = format!("FPS:{:2} | Rate:{:>6}KB/s", fps, _length_all / 1024);
                    }
                    fps = fpscount;
                    fpscount = 0;
                    _length_sum = 0;
                }
            }
            tx.send(Msg::Draw);
        }
    });

    // 主线程不断重画
    while app::wait() {
        match rx.recv() {
            Some(Msg::Draw) => {
                frame.redraw();
            }
            _ => {}
        }
    }
}

/// 发送密码，进行验证
fn validate_password(conn: &mut TcpStream, pwd: &str) -> Result<()> {
    // 验证密码
    let mut hasher = DefaultHasher::new();
    hasher.write(pwd.as_bytes());
    let pk = hasher.finish();
    conn.write_all(&pk.to_be_bytes()).unwrap();
    let mut verify_result: [u8; 1] = [0];

    //获取server的信息
    conn.read_exact(&mut verify_result).unwrap();

    // 对结果验证
    if verify_result[0] != 1 {
        if verify_result[0] == 2 {
            panic!("Password error !");
        } else {
            panic!("Some error !");
        }
    }
    Ok(())
}

/// 进行操控
/// 当遇到一个鼠标或者键盘事件，就进行发送指令给server
fn deal_with_events(w: i32, h: i32, frame: &mut Frame, txc: TcpStream) {
    let mut hooked = false;

    //用来防止一直按键
    let mut bmap = bitmap::Bitmap::new();
    let mut cmd_buf = [0u8; 5];
    let mut txc = txc;
    frame.handle(move |f, ev| {
        match ev {
            Event::Enter => {
                // 进入窗口
                hooked = true;
            }
            Event::Leave => {
                // 离开窗口
                hooked = false;
            }
            Event::KeyDown if hooked => {
                // 按键按下
                let key = app::event_key().bits() as u8;
                cmd_buf[0] = communication::KEY_DOWN;
                cmd_buf[1] = key;
                if bmap.push(key) {
                    txc.write_all(&cmd_buf[..2]).unwrap();
                }
            }
            Event::Shortcut if hooked => {
                // 按键按下
                let key = app::event_key().bits() as u8;
                cmd_buf[0] = communication::KEY_DOWN;
                cmd_buf[1] = key;
                if bmap.push(key) {
                    txc.write_all(&cmd_buf[..2]).unwrap();
                }
            }
            Event::KeyUp if hooked => {
                // 按键放开
                let key = app::event_key().bits() as u8;
                bmap.remove(key);
                cmd_buf[0] = communication::KEY_UP;
                cmd_buf[1] = key;
                txc.write_all(&cmd_buf[..2]).unwrap();
            }
            Event::Move if hooked => {
                // 鼠标移动
                let relx = (w * app::event_x() / f.width()) as u16;
                let rely = (h * app::event_y() / f.height()) as u16;
                // MOVE xu xd yu yd
                cmd_buf[0] = communication::MOVE;
                cmd_buf[1] = (relx >> 8) as u8;
                cmd_buf[2] = relx as u8;
                cmd_buf[3] = (rely >> 8) as u8;
                cmd_buf[4] = rely as u8;
                txc.write_all(&cmd_buf).unwrap();
            }
            Event::Push if hooked => {
                // 鼠标按下
                cmd_buf[0] = communication::MOUSE_KEY_DOWN;
                cmd_buf[1] = app::event_key().bits() as u8;
                txc.write_all(&cmd_buf[..2]).unwrap();
            }
            Event::Released if hooked => {
                // 鼠标释放
                cmd_buf[0] = communication::MOUSE_KEY_UP;
                cmd_buf[1] = app::event_key().bits() as u8;
                txc.write_all(&cmd_buf[..2]).unwrap();
            }
            Event::Drag if hooked => {
                // 鼠标按下移动
                let relx = (w * app::event_x() / f.width()) as u16;
                let rely = (h * app::event_y() / f.height()) as u16;
                // MOVE xu xd yu yd
                cmd_buf[0] = communication::MOVE;
                cmd_buf[1] = (relx >> 8) as u8;
                cmd_buf[2] = relx as u8;
                cmd_buf[3] = (rely >> 8) as u8;
                cmd_buf[4] = rely as u8;
                txc.write_all(&cmd_buf).unwrap();
            }
            Event::MouseWheel if hooked => {
                // app::MouseWheel::Down;
                match app::event_dy() {
                    app::MouseWheel::Down => {
                        // 滚轮下滚
                        cmd_buf[0] = communication::MOUSE_WHEEL_DOWN;
                        txc.write_all(&cmd_buf[..1]).unwrap();
                    }
                    app::MouseWheel::Up => {
                        // 滚轮上滚
                        cmd_buf[0] = communication::MOUSE_WHEEL_UP;
                        txc.write_all(&cmd_buf[..1]).unwrap();
                    }
                    _ => {}
                }
            }
            _ => {
                if hooked {
                    println!("{}", ev);
                }
            }
        }
        true
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_hash_to_bytes_big_endian() {
        let pwd = "example_password";
        let mut hasher = DefaultHasher::new();
        hasher.write(pwd.as_bytes());
        let pk = hasher.finish();
        let me = pk.to_be_bytes();

        // 这里我们需要知道这个 “example_password” 预期的哈希值的大端序字节数组表示。
        // 注意: 这个字节序列是示例，你需要根据 `calculate_hash_to_bytes` 的实际输出来替换它。
        let expected_bytes: [u8; 8] = [
            (pk >> (7 * 8)) as u8,
            (pk >> (6 * 8)) as u8,
            (pk >> (5 * 8)) as u8,
            (pk >> (4 * 8)) as u8,
            (pk >> (3 * 8)) as u8,
            (pk >> (2 * 8)) as u8,
            (pk >> (1 * 8)) as u8,
            pk as u8,
        ]; // 需要替换为正确的值

        // 确定函数输出的字节序列是否与我们预期的大端序字节序列相匹配。
        assert_eq!(me, expected_bytes);
    }
}
