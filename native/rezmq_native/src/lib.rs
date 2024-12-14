mod atoms;
mod util;

use std::{
  collections::{BTreeMap, HashSet, VecDeque},
  ffi::CString,
  io::{Read, Write},
  os::{
    fd::{AsFd, AsRawFd},
    raw::c_void,
    unix::net::UnixStream,
  },
  ptr::NonNull,
  sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
  },
};

use nix::{
  poll::{PollFd, PollFlags, PollTimeout},
  sys::socket::{AddressFamily, SockFlag, SockType},
};
use once_cell::sync::OnceCell;
use rustler::{
  Binary, Encoder, Env, LocalPid, NewBinary, OwnedBinary, OwnedEnv, Reference, Resource,
  ResourceArc, Term,
};
use util::SyncNonNull;
use zmq::{bindings, message::Message};

pub mod zmq;

static WQ: OnceCell<mpsc::Sender<Box<dyn FnOnce() + Send + Sync>>> = OnceCell::new();

struct ContextResource {
  raw: SyncNonNull<c_void>,
}

#[rustler::resource_impl]
impl Resource for ContextResource {}

impl Drop for ContextResource {
  fn drop(&mut self) {
    let raw = self.raw;
    WQ.get()
      .unwrap()
      .send(Box::new(move || unsafe {
        let raw = raw;
        bindings::zmq_ctx_term(raw.0.as_ptr());
      }))
      .unwrap();
  }
}

struct WorkerResource {
  tx: PacketTx,
}

#[rustler::resource_impl]
impl Resource for WorkerResource {}

struct Packet {
  req: Req,
}

enum Req {
  Stop {
    res: mpsc::Sender<()>,
  },
  SocketCreate {
    res: mpsc::Sender<Result<u64, i32>>,
    ty: i32,
  },
  SocketDestroy {
    id: u64,
  },
  StartRead {
    listener: LocalPid,
    token: OwnedBinary,
    id: u64,
  },
  Bind {
    id: u64,
    endpoint: String,
    res: mpsc::Sender<Result<(), i32>>,
  },
  Connect {
    id: u64,
    endpoint: String,
    res: mpsc::Sender<Result<(), i32>>,
  },
  AbortRead {
    id: u64,
  },
  Write {
    id: u64,
    message: Vec<u8>,
  },
  Setsockopt {
    id: u64,
    option_name: i32,
    option_value: Vec<u8>,
    res: mpsc::Sender<Result<(), i32>>,
  },
}

#[derive(Clone)]
struct PacketTx {
  tx: Arc<Mutex<UnixStream>>,
}

impl PacketTx {
  fn nbsend(&self, packet: Box<Packet>) -> Result<(), Box<Packet>> {
    if let Ok(mut tx) = self.tx.try_lock() {
      let packet = Box::into_raw(packet);
      if let Ok(n) = tx.write(&(packet as usize).to_ne_bytes()) {
        assert_eq!(n, std::mem::size_of::<usize>());
        Ok(())
      } else {
        Err(unsafe { Box::from_raw(packet) })
      }
    } else {
      Err(packet)
    }
  }

  fn send(&self, packet: Box<Packet>) -> Result<(), std::io::Error> {
    let mut tx = self.tx.lock().unwrap();
    let n = nix::poll::poll(
      &mut [PollFd::new(
        tx.as_fd(),
        PollFlags::POLLOUT | PollFlags::POLLERR,
      )],
      PollTimeout::NONE,
    )?;
    assert_eq!(n, 1);
    let packet = Box::into_raw(packet);
    match tx.write(&(packet as usize).to_ne_bytes()) {
      Ok(n) => {
        assert_eq!(n, std::mem::size_of::<usize>());
        Ok(())
      }
      Err(e) => {
        unsafe {
          let _ = Box::from_raw(packet);
        }
        Err(e)
      }
    }
  }
}

struct SocketResource {
  id: u64,
  destroyed: AtomicBool,
  tx: PacketTx,
}

#[rustler::resource_impl]
impl Resource for SocketResource {}

impl Drop for SocketResource {
  fn drop(&mut self) {
    if !self.destroyed.load(Ordering::Relaxed) {
      let tx = self.tx.clone();
      let packet = Box::new(Packet {
        req: Req::SocketDestroy { id: self.id },
      });
      if let Err(packet) = tx.nbsend(packet) {
        WQ.get()
          .unwrap()
          .send(Box::new(move || {
            if let Err(e) = tx.send(packet) {
              eprintln!("SocketResource::drop(): failed to send packet: {:?}", e);
            }
          }))
          .unwrap();
      }
    }
  }
}

#[rustler::nif]
fn context_new() -> ResourceArc<ContextResource> {
  let raw = unsafe { bindings::zmq_ctx_new() };
  let raw = SyncNonNull(NonNull::new(raw).unwrap());
  ResourceArc::new(ContextResource { raw })
}

#[rustler::nif(schedule = "DirtyIo")]
fn worker_start(context: ResourceArc<ContextResource>) -> ResourceArc<WorkerResource> {
  let (tx, rx) = nix::sys::socket::socketpair(
    AddressFamily::Unix,
    SockType::SeqPacket,
    None,
    SockFlag::SOCK_CLOEXEC | SockFlag::SOCK_NONBLOCK,
  )
  .expect("rezmq: start: failed to create socketpair");
  let tx = UnixStream::from(tx);
  let rx = UnixStream::from(rx);
  std::thread::Builder::new()
    .name("rezmq-worker".into())
    .spawn(move || worker(context, rx))
    .unwrap();
  ResourceArc::new(WorkerResource {
    tx: PacketTx {
      tx: Arc::new(Mutex::new(tx)),
    },
  })
}

#[rustler::nif(schedule = "DirtyIo")]
fn worker_stop(worker: ResourceArc<WorkerResource>) -> Result<rustler::Atom, rustler::Error> {
  let (tx, rx) = mpsc::channel();
  if let Err(_) = worker.tx.send(Box::new(Packet {
    req: Req::Stop { res: tx },
  })) {
    return Err(rustler::Error::RaiseAtom("socket_worker_error"));
  }
  let _ = worker
    .tx
    .tx
    .lock()
    .unwrap()
    .shutdown(std::net::Shutdown::Write);
  let rx = rx.recv();
  match rx {
    Ok(()) => Ok(atoms::ok()),
    Err(_) => Err(rustler::Error::RaiseAtom("socket_worker_error")),
  }
}

#[rustler::nif(schedule = "DirtyIo")]
fn socket_create(
  worker: ResourceArc<WorkerResource>,
  ty: i32,
) -> Result<(rustler::Atom, ResourceArc<SocketResource>), rustler::Error> {
  let (tx, rx) = mpsc::channel();
  if let Err(_) = worker.tx.send(Box::new(Packet {
    req: Req::SocketCreate { res: tx, ty },
  })) {
    return Err(rustler::Error::RaiseAtom("socket_worker_error"));
  }
  let rx = rx.recv();
  let tx = worker.tx.tx.lock().unwrap().try_clone().unwrap();
  match rx {
    Ok(Ok(id)) => Ok((
      atoms::ok(),
      ResourceArc::new(SocketResource {
        id,
        destroyed: AtomicBool::new(false),
        tx: PacketTx {
          tx: Arc::new(Mutex::new(tx)),
        },
      }),
    )),
    Ok(Err(errno)) => Err(rustler::Error::Term(Box::new(errno))),
    Err(_) => Err(rustler::Error::RaiseAtom("socket_worker_error")),
  }
}

#[rustler::nif(schedule = "DirtyIo")]
fn socket_destroy(socket: ResourceArc<SocketResource>) -> Result<rustler::Atom, rustler::Error> {
  if socket
    .destroyed
    .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
    .is_ok()
  {
    if let Err(_) = socket.tx.send(Box::new(Packet {
      req: Req::SocketDestroy { id: socket.id },
    })) {
      return Err(rustler::Error::RaiseAtom("socket_worker_error"));
    }
  }
  Ok(atoms::ok())
}

#[rustler::nif(schedule = "DirtyIo")]
fn socket_setsockopt(
  socket: ResourceArc<SocketResource>,
  option_name: i32,
  option_value: Binary,
) -> Result<rustler::Atom, rustler::Error> {
  let (tx, rx) = mpsc::channel();

  if let Err(_) = socket.tx.send(Box::new(Packet {
    req: Req::Setsockopt {
      id: socket.id,
      option_name,
      option_value: option_value.to_vec(),
      res: tx,
    },
  })) {
    return Err(rustler::Error::RaiseAtom("socket_worker_error"));
  }

  let rx = rx.recv();
  match rx {
    Ok(Ok(())) => Ok(atoms::ok()),
    Ok(Err(errno)) => Err(rustler::Error::Term(Box::new(errno))),
    Err(_) => Err(rustler::Error::RaiseAtom("socket_worker_error")),
  }
}

#[rustler::nif(schedule = "DirtyIo")]
fn socket_bind(
  socket: ResourceArc<SocketResource>,
  endpoint: String,
) -> Result<rustler::Atom, rustler::Error> {
  let (tx, rx) = mpsc::channel();

  if let Err(_) = socket.tx.send(Box::new(Packet {
    req: Req::Bind {
      id: socket.id,
      endpoint,
      res: tx,
    },
  })) {
    return Err(rustler::Error::RaiseAtom("socket_worker_error"));
  }

  let rx = rx.recv();
  match rx {
    Ok(Ok(())) => Ok(atoms::ok()),
    Ok(Err(errno)) => Err(rustler::Error::Term(Box::new(errno))),
    Err(_) => Err(rustler::Error::RaiseAtom("socket_worker_error")),
  }
}

#[rustler::nif(schedule = "DirtyIo")]
fn socket_connect(
  socket: ResourceArc<SocketResource>,
  endpoint: String,
) -> Result<rustler::Atom, rustler::Error> {
  let (tx, rx) = mpsc::channel();

  if let Err(_) = socket.tx.send(Box::new(Packet {
    req: Req::Connect {
      id: socket.id,
      endpoint,
      res: tx,
    },
  })) {
    return Err(rustler::Error::RaiseAtom("socket_worker_error"));
  }

  let rx = rx.recv();
  match rx {
    Ok(Ok(())) => Ok(atoms::ok()),
    Ok(Err(errno)) => Err(rustler::Error::Term(Box::new(errno))),
    Err(_) => Err(rustler::Error::RaiseAtom("socket_worker_error")),
  }
}

#[rustler::nif(schedule = "DirtyIo")]
fn socket_start_read(
  socket: ResourceArc<SocketResource>,
  listener: LocalPid,
  token: Reference,
) -> Result<rustler::Atom, rustler::Error> {
  if let Err(_) = socket.tx.send(Box::new(Packet {
    req: Req::StartRead {
      listener,
      token: token.to_binary(),
      id: socket.id,
    },
  })) {
    return Err(rustler::Error::RaiseAtom("socket_worker_error"));
  }

  Ok(atoms::ok())
}

#[rustler::nif(schedule = "DirtyIo")]
fn socket_abort_read(socket: ResourceArc<SocketResource>) -> Result<rustler::Atom, rustler::Error> {
  if let Err(_) = socket.tx.send(Box::new(Packet {
    req: Req::AbortRead { id: socket.id },
  })) {
    return Err(rustler::Error::RaiseAtom("socket_worker_error"));
  }

  Ok(atoms::ok())
}

#[rustler::nif(schedule = "DirtyIo")]
fn socket_write_encoded(
  socket: ResourceArc<SocketResource>,
  message: Binary,
) -> Result<rustler::Atom, rustler::Error> {
  if socket.destroyed.load(Ordering::Relaxed) {
    return Err(rustler::Error::Term(Box::new(atoms::socket_destroyed())));
  }

  if let Err(_) = socket.tx.send(Box::new(Packet {
    req: Req::Write {
      id: socket.id,
      message: message.to_vec(),
    },
  })) {
    return Err(rustler::Error::RaiseAtom("socket_worker_error"));
  }

  Ok(atoms::ok())
}

#[rustler::nif]
fn socket_write_encoded_fast(
  socket: ResourceArc<SocketResource>,
  message: Binary,
) -> rustler::Atom {
  if socket.destroyed.load(Ordering::Relaxed) {
    return atoms::error();
  }

  if message.len() > 65536 {
    return atoms::would_block();
  }

  if let Err(_) = socket.tx.nbsend(Box::new(Packet {
    req: Req::Write {
      id: socket.id,
      message: message.to_vec(),
    },
  })) {
    atoms::would_block()
  } else {
    atoms::ok()
  }
}

struct OwnedSocket(NonNull<c_void>);
impl Drop for OwnedSocket {
  fn drop(&mut self) {
    unsafe {
      bindings::zmq_close(self.0.as_ptr());
    }
  }
}

fn worker(ctx: ResourceArc<ContextResource>, mut rx: UnixStream) {
  let mut env = OwnedEnv::new();
  let mut next_socket_id = 1u64;
  let mut sockets: BTreeMap<u64, OwnedSocket> = BTreeMap::new();
  let mut reading: BTreeMap<u64, (LocalPid, OwnedBinary, Vec<Message>)> = BTreeMap::new();
  let mut writing: BTreeMap<u64, VecDeque<(Message, bool)>> = BTreeMap::new();

  loop {
    let n = 1 + reading.len() + writing.len();
    let mut poll_items: Vec<bindings::zmq_pollitem_t> = Vec::with_capacity(n);
    poll_items.push(bindings::zmq_pollitem_t {
      socket: std::ptr::null_mut(),
      events: (bindings::ZMQ_POLLIN | bindings::ZMQ_POLLERR) as i16,
      revents: 0,
      fd: rx.as_raw_fd(),
    });
    for (socket_id, _) in &reading {
      let socket = sockets.get(socket_id).unwrap();
      poll_items.push(bindings::zmq_pollitem_t {
        socket: socket.0.as_ptr(),
        events: (bindings::ZMQ_POLLIN | bindings::ZMQ_POLLERR) as i16,
        revents: 0,
        fd: 0,
      });
    }
    for (socket_id, _) in &writing {
      let socket = sockets.get(socket_id).unwrap();
      poll_items.push(bindings::zmq_pollitem_t {
        socket: socket.0.as_ptr(),
        events: (bindings::ZMQ_POLLOUT | bindings::ZMQ_POLLERR) as i16,
        revents: 0,
        fd: 0,
      });
    }
    assert_eq!(poll_items.len(), n);
    let n = unsafe { bindings::zmq_poll(poll_items.as_mut_ptr(), n as i32, -1) };
    assert!(n > 0);

    let mut reading_to_remove: HashSet<u64> = HashSet::new();

    for (item, (socket_id, (caller, token, buf))) in poll_items[1..1 + reading.len()]
      .iter()
      .zip(reading.iter_mut())
    {
      let mut respond = |data: Result<Vec<Message>, i32>| {
        let _ = env.send_and_clear(caller, |env| {
          // safe because term is created by us
          let token = unsafe { env.binary_to_term_trusted(&token).unwrap().0 };

          match data {
            Ok(msg) => {
              let mut encoded: Vec<Binary> = Vec::with_capacity(msg.len());
              for msg in &msg {
                let mut b = NewBinary::new(env, msg.len());
                b.copy_from_slice(&msg[..]);
                encoded.push(Binary::from(b));
              }
              (atoms::rezmq_msg(), token, atoms::ok(), encoded).encode(env)
            }
            Err(code) => (atoms::rezmq_msg(), token, atoms::error(), code).encode(env),
          }
        });
      };
      if item.revents & bindings::ZMQ_POLLERR as i16 != 0 {
        respond(Err(0));
        reading_to_remove.insert(*socket_id);
      } else if item.revents & bindings::ZMQ_POLLIN as i16 != 0 {
        let mut msg = Message::new();
        let ret = unsafe {
          bindings::zmq_msg_recv(
            &mut msg.msg,
            sockets.get(socket_id).unwrap().0.as_ptr(),
            bindings::ZMQ_DONTWAIT as i32,
          )
        };
        if ret < 0 {
          let errno = unsafe { bindings::zmq_errno() };

          // spurious EAGAIN
          if errno != libc::EAGAIN {
            respond(Err(errno));
            reading_to_remove.insert(*socket_id);
          }
        } else {
          let more = msg.get_more();
          buf.push(msg);
          if !more {
            respond(Ok(std::mem::take(buf)));
          }
        }
      }
    }

    let mut writing_to_remove = HashSet::new();

    for (item, (socket_id, pending_messages)) in poll_items[1 + reading.len()..]
      .iter()
      .zip(writing.iter_mut())
    {
      if item.revents & bindings::ZMQ_POLLERR as i16 != 0 {
        writing_to_remove.insert(*socket_id);
      } else if item.revents & bindings::ZMQ_POLLOUT as i16 != 0 {
        let (mut msg, more) = pending_messages.pop_front().unwrap();
        let socket = sockets.get(socket_id).unwrap();
        let send = |msg: &mut Message, more: bool| unsafe {
          bindings::zmq_msg_send(
            &mut msg.msg,
            socket.0.as_ptr(),
            bindings::ZMQ_DONTWAIT as i32
              | (if more {
                bindings::ZMQ_SNDMORE as i32
              } else {
                0
              }),
          )
        };
        if send(&mut msg, more) < 0 || pending_messages.is_empty() {
          writing_to_remove.insert(*socket_id);
        } else {
          // best-effort: write more messages
          while let Some((msg, more)) = pending_messages.front_mut() {
            if send(msg, *more) < 0 {
              break;
            }
            pending_messages.pop_front().unwrap();
          }
          if pending_messages.is_empty() {
            writing_to_remove.insert(*socket_id);
          }
        }
      }
    }

    for x in &reading_to_remove {
      reading.remove(x);
    }
    for x in &writing_to_remove {
      writing.remove(x);
    }

    if poll_items[0].revents != 0 {
      if poll_items[0].revents & bindings::ZMQ_POLLERR as i16 != 0 {
        break;
      }

      if poll_items[0].revents & bindings::ZMQ_POLLIN as i16 == 0 {
        panic!("unexpected revents on rx: {}", poll_items[0].revents);
      }

      let mut packet_box = [0u8; std::mem::size_of::<usize>()];
      let n = rx.read(&mut packet_box).unwrap();
      if n == 0 {
        break;
      }
      assert_eq!(n, std::mem::size_of::<usize>());
      let packet: Box<Packet> =
        unsafe { Box::from_raw(usize::from_ne_bytes(packet_box) as *mut Packet) };

      match packet.req {
        Req::Stop { res } => {
          // drop all sockets, consume rx to end, return
          drop(sockets);
          let _ = res.send(());
          loop {
            let n = nix::poll::poll(
              &mut [PollFd::new(
                rx.as_fd(),
                PollFlags::POLLIN | PollFlags::POLLERR,
              )],
              PollTimeout::NONE,
            )
            .unwrap();
            assert!(n == 1);
            let n = rx.read(&mut packet_box).unwrap();
            if n == 0 {
              break;
            }

            assert_eq!(n, std::mem::size_of::<usize>());
            let _: Box<Packet> =
              unsafe { Box::from_raw(usize::from_ne_bytes(packet_box) as *mut Packet) };
          }
          break;
        }
        Req::SocketCreate { res, ty } => {
          let socket = unsafe { bindings::zmq_socket(ctx.raw.0.as_ptr(), ty) };
          if let Some(socket) = NonNull::new(socket) {
            let id = next_socket_id;
            next_socket_id += 1;
            sockets.insert(id, OwnedSocket(socket));
            let _ = res.send(Ok(id));
          } else {
            let errno = unsafe { bindings::zmq_errno() };
            let _ = res.send(Err(errno));
          }
        }
        Req::SocketDestroy { id } => {
          if sockets.remove(&id).is_some() {
            reading.remove(&id);
            writing.remove(&id);
          }
        }
        Req::Bind { id, endpoint, res } => {
          if let Some(x) = sockets.get(&id) {
            if let Ok(endpoint) = CString::new(endpoint) {
              let ret = unsafe { bindings::zmq_bind(x.0.as_ptr(), endpoint.as_ptr()) };
              if ret < 0 {
                let errno = unsafe { bindings::zmq_errno() };
                let _ = res.send(Err(errno));
              } else {
                let _ = res.send(Ok(()));
              }
            }
          }
        }
        Req::Connect { id, endpoint, res } => {
          if let Some(x) = sockets.get(&id) {
            if let Ok(endpoint) = CString::new(endpoint) {
              let ret = unsafe { bindings::zmq_connect(x.0.as_ptr(), endpoint.as_ptr()) };
              if ret < 0 {
                let errno = unsafe { bindings::zmq_errno() };
                let _ = res.send(Err(errno));
              } else {
                let _ = res.send(Ok(()));
              }
            }
          }
        }
        Req::StartRead {
          listener,
          token,
          id,
        } => {
          if reading.contains_key(&id) {
            let _ = env.send_and_clear(&listener, |env| {
              let (token, _) = env.binary_to_term(&token).unwrap();
              (
                atoms::rezmq_msg(),
                token,
                atoms::error(),
                atoms::already_reading(),
              )
            });
          } else if !sockets.contains_key(&id) {
            let _ = env.send_and_clear(&listener, |env| {
              let (token, _) = env.binary_to_term(&token).unwrap();
              (
                atoms::rezmq_msg(),
                token,
                atoms::error(),
                atoms::not_found(),
              )
            });
          } else {
            reading.insert(id, (listener, token, Vec::new()));
          }
        }
        Req::AbortRead { id } => {
          reading.remove(&id);
        }
        Req::Write { id, message } => {
          if sockets.contains_key(&id) {
            let ret = env.run(|env| {
              let (term, _) = env.binary_to_term(&message)?;
              let list_length = term.list_length().ok()?;

              let q = writing.entry(id).or_default();
              q.reserve(list_length);
              q.extend(term.into_list_iterator().ok()?.enumerate().map(|(i, x)| {
                let x = x.into_binary().ok();
                let msg = x.as_ref().map(|x| &x[..]).unwrap_or(b"");
                let more = i + 1 != list_length;
                (Message::from(msg), more)
              }));

              Some(())
            });
            env.clear();

            if ret.is_none() {
              eprintln!("rezmq_native: invalid async write");
            }
          }
        }
        Req::Setsockopt {
          id,
          option_name,
          option_value,
          res,
        } => {
          if let Some(socket) = sockets.get(&id) {
            let ret = unsafe {
              bindings::zmq_setsockopt(
                socket.0.as_ptr(),
                option_name,
                option_value.as_ptr() as *const c_void,
                option_value.len(),
              )
            };
            if ret < 0 {
              let _ = res.send(Err(unsafe { bindings::zmq_errno() }));
            } else {
              let _ = res.send(Ok(()));
            }
          }
        }
      }
    }
  }
}

fn load(_env: Env, _: Term) -> bool {
  let (tx, rx) = mpsc::channel();
  WQ.set(tx).unwrap();
  std::thread::Builder::new()
    .name("rezmq-wq".into())
    .spawn(move || loop {
      let Ok(work) = rx.recv() else {
        break;
      };
      work();
    })
    .unwrap();
  true
}

rustler::init!("Elixir.Rezmq.Native", load = load);
