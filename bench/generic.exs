ctx = Rezmq.context_new()
worker = Rezmq.worker_start(ctx)

spawn_link(fn ->
  {:ok, socket} = Rezmq.socket_create(worker, :router)
  :ok = Rezmq.socket_bind(socket, "inproc://echo_server")
  token = make_ref()
  :ok = Rezmq.socket_start_read(socket, self(), token)

  Stream.iterate(nil, fn nil ->
    receive do
      {:rezmq_msg, ^token, :ok, [identity, req_id, "" | data]} ->
        :ok = Rezmq.socket_write(socket, [identity, req_id, "" | data])

      x ->
        raise "unexpected message (rep): #{inspect(x)}"
    end

    nil
  end)
  |> Enum.count()

  nil
end)

gen_roundtrip = fn worker ->
  {:ok, socket} = Rezmq.socket_create(worker, :req)

  :ok =
    Rezmq.socket_setsockopt(
      socket,
      :req_relaxed,
      <<1::32-little>>
    )

  :ok =
    Rezmq.socket_setsockopt(
      socket,
      :req_correlate,
      <<1::32-little>>
    )

  :ok = Rezmq.socket_connect(socket, "inproc://echo_server")
  token = make_ref()

  fn ->
    if Process.get(:started_read) == nil do
      :ok = Rezmq.socket_start_read(socket, self(), token)
      Process.put(:started_read, true)
    end

    :ok = Rezmq.socket_write(socket, ["hello"])

    receive do
      {:rezmq_msg, ^token, :ok, ["hello"]} ->
        nil

      x ->
        raise "unexpected message: #{inspect(x)}"
    after
      1000 -> raise "timeout"
    end
  end
end

Benchee.run(%{
  "create_socket" => fn ->
    {:ok, socket} = Rezmq.socket_create(worker, :req)
    :ok = Rezmq.socket_destroy(socket)
  end,
  "start_and_stop_worker" => fn ->
    worker = Rezmq.worker_start(ctx)
    :ok = Rezmq.worker_stop(worker)
  end,
  "message_roundtrip_same_worker" => gen_roundtrip.(worker),
  "message_roundtrip_different_worker" => gen_roundtrip.(Rezmq.worker_start(ctx))
})
