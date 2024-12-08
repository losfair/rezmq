# Rezmq

Fast ZeroMQ bindings for Elixir.

`rezmq` handles I/O multiplexing for ZMQ sockets on a background threads using
`zmq_poll`, takes care not to block the BEAM scheduler, and does not use dirty
NIF functions on the hot path.

## Usage

```elixir
ctx = Rezmq.context_new()

# A worker is a background thread that handles I/O
# Multiple sockets can be created on the same worker
worker = Rezmq.worker_start(ctx)

{:ok, socket} = Rezmq.socket_create(worker, :req)
:ok = Rezmq.socket_setsockopt(socket, :req_relaxed, <<1::32-little>>)
:ok = Rezmq.socket_setsockopt(socket, :req_correlate, <<1::32-little>>)
:ok = Rezmq.socket_connect(socket, "tcp://127.0.0.1:3333")

# `token = socket` also works because `socket` is a reference
token = make_ref()

# This tells rezmq to send all received ZMQ messages to `self()`
:ok = Rezmq.socket_start_read(socket, self(), token)
:ok = Rezmq.socket_write(socket, ["hello"])
receive do
  {:rezmq_msg, ^token, :ok, msg} -> IO.inspect(msg)
  x -> raise "unexpected message: #{inspect(x)}"
end
```

## Benchmark

```bash
# rezmq
$ mix run bench/echo.exs 
Duration: 30.91 seconds
Requests per second: 77644.77515367194

# erlzmq_dnif
# `+SDio 200` required - otherwise deadlocks
$ ELIXIR_ERL_OPTIONS="+SDio 200" mix run bench/erlzmq_dnif_echo.exs 
Duration: 24.635 seconds
Requests per second: 4871.118327582707
```
