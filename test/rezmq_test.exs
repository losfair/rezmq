defmodule RezmqTest do
  use ExUnit.Case
  doctest Rezmq

  test "fast and normal writes" do
    ctx = Rezmq.context_new()
    worker = Rezmq.worker_start(ctx)
    {:ok, socket} = Rezmq.socket_create(worker, :req)
    :ok = Rezmq.socket_connect(socket, "inproc://badaddr")

    small_bytes_encoded = :erlang.term_to_binary([<<0::big-(65000 * 8)>>])
    large_bytes_encoded = :erlang.term_to_binary([<<0::big-(700_000 * 8)>>])
    start_time = :erlang.monotonic_time(:microsecond)

    :ok =
      Rezmq.Native.socket_write_encoded_fast(
        socket,
        small_bytes_encoded
      )

    end_time = :erlang.monotonic_time(:microsecond)
    IO.puts("fast write (accepted): #{end_time - start_time}us")

    start_time = :erlang.monotonic_time(:microsecond)

    :would_block =
      Rezmq.Native.socket_write_encoded_fast(
        socket,
        large_bytes_encoded
      )

    end_time = :erlang.monotonic_time(:microsecond)
    IO.puts("fast write (rejected): #{end_time - start_time}us")

    nil
  end

  test "socket use after destroy" do
    ctx = Rezmq.context_new()
    worker = Rezmq.worker_start(ctx)
    {:ok, socket} = Rezmq.socket_create(worker, :req)
    :ok = Rezmq.socket_connect(socket, "inproc://badaddr")
    :ok = Rezmq.socket_write(socket, ["abc"])
    :ok = Rezmq.socket_destroy(socket)
    {:error, :socket_destroyed} = Rezmq.socket_write(socket, ["abc"])
    nil
  end

  test "socket use after worker is stopped" do
    ctx = Rezmq.context_new()
    worker = Rezmq.worker_start(ctx)
    {:ok, socket} = Rezmq.socket_create(worker, :req)
    :ok = Rezmq.socket_connect(socket, "inproc://badaddr")
    :ok = Rezmq.socket_write(socket, ["abc"])
    :ok = Rezmq.worker_stop(worker)

    try do
      Rezmq.socket_write(socket, ["abc"])
      raise("expected error")
    catch
      :error, :socket_worker_error -> nil
    end

    nil
  end

  test "rebind after process exit" do
    ctx = Rezmq.context_new()
    worker = Rezmq.worker_start(ctx)

    me = self()

    pid =
      spawn_link(fn ->
        {:ok, socket} = Rezmq.socket_create(worker, :rep)
        :ok = Rezmq.socket_bind(socket, "inproc://addr1")
        send(me, :ready)

        receive do
          x -> raise "unexpected message: #{inspect(x)}"
        end
      end)

    receive do
      :ready -> nil
      x -> raise "unexpected message: #{inspect(x)}"
    end

    {:ok, socket} = Rezmq.socket_create(worker, :rep)
    {:error, 98} = Rezmq.socket_bind(socket, "inproc://addr1")

    Process.unlink(pid)
    Process.exit(pid, :kill)
    Process.sleep(10)
    :ok = Rezmq.socket_bind(socket, "inproc://addr1")

    nil
  end

  test "message metadata" do
    ctx = Rezmq.context_new()
    worker = Rezmq.worker_start(ctx)
    {:ok, req} = Rezmq.socket_create(worker, :req)
    {:ok, rep} = Rezmq.socket_create(worker, :rep)

    # message metadata doesn't work with inproc
    tempfile = "ipc:///tmp/rezmq_message_metadata_test.sock"
    :ok = Rezmq.socket_bind(rep, tempfile)
    :ok = Rezmq.socket_connect(req, tempfile)

    child =
      spawn_link(fn ->
        :ok = Rezmq.socket_start_read(rep, self(), rep, {"Socket-Type"})

        Stream.repeatedly(fn ->
          receive do
            {:rezmq_msg, ^rep, :ok, {["hello"], {x}}} ->
              :ok = Rezmq.socket_write(rep, [inspect(x)])
              nil

            :exit ->
              Process.exit(self(), :normal)

            x ->
              raise "unexpected message: #{inspect(x)}"
          end
        end)
        |> Enum.each(& &1)
      end)

    :ok = Rezmq.socket_start_read(req, self(), req)
    :ok = Rezmq.socket_write(req, ["hello"])

    md_entry =
      receive do
        {:rezmq_msg, ^req, :ok, [x]} -> x
        x -> raise "unexpected message: #{inspect(x)}"
      end

    assert md_entry == "\"REQ\""
    send(child, :exit)
    nil
  end
end
