defmodule Rezmq.ReqMultiplexer do
  require Logger
  use GenServer

  @impl true
  def init(args) do
    socket = Keyword.fetch!(args, :dealer_socket)
    inflight = :ets.new(nil, [])
    :ets.insert(inflight, {:current_id, 0})
    Rezmq.socket_start_read(socket, self(), socket)

    {:ok,
     %{
       socket: socket,
       inflight: inflight
     }}
  end

  @impl true
  def handle_call({:req, message}, from, %{socket: socket, inflight: inflight} = state) do
    {pid, _} = from
    mon = Process.monitor(pid)

    req_id = Integer.to_string(:ets.update_counter(inflight, :current_id, 1))

    :ets.insert(
      inflight,
      [
        {{:mon, mon}, req_id},
        {{:req, req_id}, {mon, from}}
      ]
    )

    Rezmq.socket_write(socket, [req_id, "" | message])

    {:noreply, state}
  end

  @impl true
  def handle_info(
        {:DOWN, mon, :process, _pid, _reason},
        %{inflight: inflight} = st
      ) do
    [{_, req_id}] = :ets.lookup(inflight, {:mon, mon})
    [{_, _}] = :ets.lookup(inflight, {:req, req_id})

    :ets.delete(inflight, {:mon, mon})
    :ets.delete(inflight, {:req, req_id})
    {:noreply, st}
  end

  @impl true
  def handle_info({:rezmq_msg, socket_, status, msg}, %{socket: socket, inflight: inflight} = st)
      when socket_ === socket do
    case status do
      :ok -> nil
      :error -> raise "ReqMultiplexer error: #{inspect(msg)}"
    end

    entry =
      case msg do
        [req_id, "" | _] -> :ets.lookup(inflight, {:req, req_id})
        _ -> []
      end

    case entry do
      [{{:req, req_id}, {mon, from}}] ->
        Process.demonitor(mon)
        [{_, _}] = :ets.lookup(inflight, {:mon, mon})
        :ets.delete(inflight, {:mon, mon})
        :ets.delete(inflight, {:req, req_id})
        GenServer.reply(from, Enum.drop(msg, 2))

      [] ->
        nil
    end

    {:noreply, st}
  end
end
