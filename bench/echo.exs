me = self()
ctx = Rezmq.context_new()

num_servers = 8

Enum.each(1..num_servers, fn i ->
  spawn_link(fn ->
    worker = Rezmq.worker_start(ctx)
    {:ok, socket} = Rezmq.socket_create(worker, :rep)
    :ok = Rezmq.socket_bind(socket, "inproc://echo_server_#{i}")
    token = make_ref()
    :ok = Rezmq.socket_start_read(socket, self(), token)

    Stream.repeatedly(fn ->
      receive do
        {:rezmq_msg, ^token, :ok, data} ->
          # IO.puts("REP: #{inspect(data)}")
          :ok = Rezmq.socket_write(socket, data)

        x ->
          raise "unexpected message (rep): #{inspect(x)}"
      end

      nil
    end)
    |> Enum.each(& &1)

    nil
  end)
end)

n = 240
m = 10000
num_workers = 8

# from a different worker
workers = 1..num_workers |> Enum.map(fn _ -> Rezmq.worker_start(ctx) end)

1..n
|> Enum.each(fn i ->
  worker = Enum.at(workers, rem(i, num_workers))

  spawn_link(fn ->
    {:ok, socket} = Rezmq.socket_create(worker, :dealer)

    Enum.each(1..num_servers, fn i ->
      :ok = Rezmq.socket_connect(socket, "inproc://echo_server_#{i}")
    end)

    token = make_ref()
    :ok = Rezmq.socket_start_read(socket, self(), token)

    1..m
    |> Enum.each(fn i ->
      i = Integer.to_string(i)
      :ok = Rezmq.socket_write(socket, ["", "worker", i])

      receive do
        {:rezmq_msg, ^token, :ok, ["", "worker", ^i]} ->
          nil

        x ->
          raise "unexpected message (worker1): #{inspect(x)}"
      end
    end)

    send(me, Ready)
    nil
  end)
end)

start_time = DateTime.to_unix(DateTime.utc_now(), :millisecond)

1..n
|> Enum.each(fn _ ->
  receive do
    Ready ->
      nil
  end
end)

end_time = DateTime.to_unix(DateTime.utc_now(), :millisecond)

duration_secs = (end_time - start_time) / 1000
messages_per_sec = n * m / duration_secs

IO.puts("Duration: #{duration_secs} seconds")
IO.puts("Requests per second: #{messages_per_sec}")
