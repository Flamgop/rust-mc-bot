mod packet_utils;
mod packet_processors;
mod net;
mod states;

use std::{net::ToSocketAddrs, env};
use std::io;
use mio::{Poll, Events, Token, Interest, event, Registry};
use std::net::SocketAddr;
use states::play;
use std::collections::HashMap;
use mio::net::TcpStream;
use states::login;
use packet_utils::Buf;
use std::time::{Duration, Instant};
use std::io::{Read, Write};
use libdeflater::{CompressionLvl, Compressor, Decompressor};

#[cfg(unix)]
use {mio::net::UnixStream, std::path::PathBuf};

const SHOULD_MOVE: bool = true;

const PROTOCOL_VERSION: u32 = 758;

#[cfg(unix)]
const UDS_PREFIX : &str = "unix://";

type Error = Box<dyn std::error::Error + Send + Sync>;

fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        let name = args.get(0).unwrap();
        #[cfg(unix)]
        println!("usage: {} <ip:port or path> <count> [threads] [spam text] [ticks between chat messages]", name);
        #[cfg(not(unix))]
        println!("usage: {} <ip:port> <count> [threads] [spam text] [time between chat messages]", name);
        println!("example: {} localhost:25565 500", name);
        #[cfg(unix)]
        println!("example: {} unix:///path/to/socket 500", name);
        return Ok(());
    }

    let arg1 = args.get(1).unwrap();
    let arg2 = args.get(2).unwrap();
    let arg3 = args.get(3);
    let arg4 = args.get(4);
    let arg5 = args.get(5);

    let mut addrs = None;

    #[cfg(unix)]
    if arg1.starts_with(UDS_PREFIX) {
        addrs = Some(Address::UNIX(PathBuf::from(arg1[UDS_PREFIX.len()..].to_owned())));
    }

    if addrs.is_none() {
        let mut parts = arg1.split(":");
        let ip = parts.next().expect("no ip provided");
        let port = parts.next().map(|port_string| port_string.parse().expect("invalid port")).unwrap_or(25565u16);

        let server = (ip, port).to_socket_addrs().expect("Not a socket address").next().expect("No socket address found");

        addrs = Some(Address::TCP(server));
    }

    // Cant be none because it would have panicked earlier
    let addrs = addrs.unwrap();

    let count: u32 = arg2.parse().expect(&format!("{} is not a number", arg2));
    let mut cpus = 1.max(num_cpus::get()) as u32;
    let mut time_between_messages = 20; // one second
    let mut spam_text = "".to_owned();

    if let Option::Some(str) = arg3 {
        cpus = str.parse().expect(&format!("{} is not a number", arg2));
    }

    if let Option::Some(str) = arg4 {
        spam_text = str.chars().take(255).collect();
    }

    if let Option::Some(str) = arg5 {
        time_between_messages = str.parse().expect(&format!("{} is not a number", arg5.unwrap()));
    }

    println!("cpus: {}", cpus);
    println!("message frequency (in ticks): {}", time_between_messages);
    if spam_text != "" {
        println!("spam text: {}", spam_text);
    } else {
        println!("No spam text!");
    }

    let count_per_thread = count/cpus;
    let mut extra = count%cpus;
    let mut names_used = 0;

    if count > 0 {
        let mut threads = Vec::new();
        for _ in 0..cpus {
            let mut count = count_per_thread;
            let spam = spam_text.clone();

            if extra > 0 {
                extra -= 1;
                count += 1;
            }

            let addrs = addrs.clone();
            threads.push(std::thread::spawn(move || { start_bots(count, addrs, names_used, cpus,spam.to_owned(), time_between_messages) }));

            names_used += count;
        }

        for thread in threads {
            let _ = thread.join();
        }
    }
    Ok(())
}

pub struct Compression {
    compressor: Compressor,
    decompressor: Decompressor,
}

pub struct Bot {
    pub token : Token,
    pub stream : Stream,
    pub name : String,
    pub compression_threshold: i32,
    pub state: u8,
    pub kicked : bool,
    pub teleported : bool,
    pub x : f64,
    pub y : f64,
    pub z : f64,
    pub buffering_buf : Buf,
    pub joined : bool
}

pub fn start_bots(count : u32, addrs : Address, name_offset : u32, cpus: u32, spam_text: String, time_between_messages: u32) {
    if count == 0 {
        return;
    }
    let mut poll = Poll::new().expect("could not unwrap poll");
    //todo check used cap
    let mut events = Events::with_capacity((count * 5) as usize);
    let mut map = HashMap::new();

    fn start_bot(bot: &mut Bot, compression: &mut Compression) {
        bot.joined = true;
        //login sequence
        let buf = login::write_handshake_packet(PROTOCOL_VERSION, "".to_string(), 0, 2);
        bot.send_packet(buf, compression);

        let buf = login::write_login_start_packet(&bot.name);
        bot.send_packet(buf, compression);

        let buf =

        println!("bot \"{}\" joined", bot.name);
    }

    let bots_per_tick = (1.0/cpus as f64).ceil() as u32;
    let mut bots_joined = 0;

    let mut packet_buf = Buf::with_length(2000);
    let mut uncompressed_buf = Buf::with_length(2000);

    let mut compression = Compression { compressor: Compressor::new(CompressionLvl::fastest()), decompressor: Decompressor::new() };

    let dur = Duration::from_millis(50);
    let mut ticks_since_last_message: u32 = 0;

    'main: loop {
        ticks_since_last_message += 1;
        if bots_joined < count {
            let registry = poll.registry();
            for bot in bots_joined..(bots_per_tick + bots_joined).min(count) {
                let token = Token(bot as usize);
                let name = "Bot_".to_owned() + &(name_offset + bot).to_string();

                let mut bot = Bot { token, stream : addrs.connect(), name, compression_threshold: 0, state: 0, kicked: false, teleported: false, x: 0.0, y: 0.0, z: 0.0, buffering_buf: Buf::with_length(200), joined : false };
                registry.register(&mut bot.stream, bot.token, Interest::READABLE | Interest::WRITABLE).expect("could not register");

                map.insert(token, bot);

                bots_joined += 1;
            }
        }

        let ins = Instant::now();
        poll.poll(&mut events, Some(dur)).expect("couldn't poll");
        for event in events.iter() {
            if let Some(bot) = map.get_mut(&event.token()) {
                if event.is_writable() && !bot.joined {
                    start_bot(bot, &mut compression);
                }
                if event.is_readable() && bot.joined {
                    net::process_packet(bot, &mut packet_buf, &mut uncompressed_buf, &mut compression);
                    if bot.kicked {
                        println!("{} disconnected", bot.name);
                        let token = bot.token;
                        map.remove(&token).expect("kicked bot doesn't exist");

                        if map.is_empty() {
                            break 'main;
                        }
                    }
                }
            }
        }

        let elapsed = ins.elapsed();
        if elapsed < dur {
            std::thread::sleep(dur-elapsed);
        }

        let mut to_remove = Vec::new();

        for bot in map.values_mut() {
            if SHOULD_MOVE && bot.teleported {
                bot.x += rand::random::<f64>()*1.0-0.5;
                bot.z += rand::random::<f64>()*1.0-0.5;
                bot.send_packet(play::write_current_pos(bot), &mut compression);
                if spam_text.len() > 0 && ticks_since_last_message >= time_between_messages {
                    ticks_since_last_message = 0;
                    bot.send_packet(play::write_chat_message(&spam_text), &mut compression);
                }
            }
            if bot.kicked {
                to_remove.push(bot.token);
            }
        }

        for bot in to_remove {
            let _ = map.remove(&bot);
        }
    }
}

#[derive(Clone)]
pub enum Address {
    #[cfg(unix)]
    UNIX(PathBuf),
    TCP(SocketAddr)
}

impl Address {
    pub fn connect(&self) -> Stream {
        match self {
            #[cfg(unix)]
            Address::UNIX(path) => {
                Stream::UNIX(UnixStream::connect(path).expect("Could not connect to the server"))
            }
            Address::TCP(address) => {
                Stream::TCP(TcpStream::connect(address.to_owned()).expect("Could not connect to the server"))
            }
        }
    }
}

pub enum Stream {
    #[cfg(unix)]
    UNIX(UnixStream),
    TCP(TcpStream)
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            #[cfg(unix)]
            Stream::UNIX(s) => {
                s.read(buf)
            }
            Stream::TCP(s) => {
                s.read(buf)
            }
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            #[cfg(unix)]
            Stream::UNIX(s) => {
                s.write(buf)
            }
            Stream::TCP(s) => {
                s.write(buf)
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            #[cfg(unix)]
            Stream::UNIX(s) => {
                s.flush()
            }
            Stream::TCP(s) => {
                s.flush()
            }
        }
    }
}

impl event::Source for Stream {
    fn register(&mut self, registry: &Registry, token: Token, interests: Interest) -> io::Result<()> {
        match self {
            #[cfg(unix)]
            Stream::UNIX(s) => {
                s.register(registry, token, interests)
            }
            Stream::TCP(s) => {
                s.register(registry, token, interests)
            }
        }
    }

    fn reregister(&mut self, registry: &Registry, token: Token, interests: Interest) -> io::Result<()> {
        match self {
            #[cfg(unix)]
            Stream::UNIX(s) => {
                s.reregister(registry, token, interests)
            }
            Stream::TCP(s) => {
                s.reregister(registry, token, interests)
            }
        }
    }

    fn deregister(&mut self, registry: &Registry) -> io::Result<()> {
        match self {
            #[cfg(unix)]
            Stream::UNIX(s) => {
                s.deregister(registry)
            }
            Stream::TCP(s) => {
                s.deregister(registry)
            }
        }
    }
}