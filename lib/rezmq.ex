defmodule Rezmq do
  @moduledoc """
  Documentation for `Rezmq`.
  """

  @spec context_new() :: Rezmq.Native.native_context()
  def context_new() do
    Rezmq.Native.context_new()
  end

  @spec worker_start(Rezmq.Native.native_context()) :: Rezmq.Native.native_worker()
  def worker_start(context) do
    Rezmq.Native.worker_start(context)
  end

  @spec worker_stop(Rezmq.Native.native_worker()) :: :ok
  def worker_stop(worker) do
    Rezmq.Native.worker_stop(worker)
  end

  @spec socket_create(Rezmq.Native.native_worker(), atom()) ::
          {:ok, Rezmq.Native.native_socket()} | {:error, integer()}
  def socket_create(worker, socket_type) do
    socket_type = Rezmq.Consts.socket_type_to_id(socket_type)
    Rezmq.Native.socket_create(worker, socket_type)
  end

  @spec socket_destroy(Rezmq.Native.native_socket()) :: :ok
  def socket_destroy(socket) do
    Rezmq.Native.socket_destroy(socket)
  end

  @spec socket_setsockopt(Rezmq.Native.native_socket(), atom(), binary()) ::
          :ok | {:error, integer()}
  def socket_setsockopt(socket, option_name, option_value) do
    option_name = Rezmq.Consts.option_name_to_id(option_name)
    Rezmq.Native.socket_setsockopt(socket, option_name, option_value)
  end

  @spec socket_bind(Rezmq.Native.native_socket(), binary()) :: :ok | {:error, integer()}
  def socket_bind(socket, address) do
    Rezmq.Native.socket_bind(socket, address)
  end

  @spec socket_connect(Rezmq.Native.native_socket(), binary()) :: :ok | {:error, integer()}
  def socket_connect(socket, address) do
    Rezmq.Native.socket_connect(socket, address)
  end

  @spec socket_start_read(Rezmq.Native.native_socket(), pid(), reference()) :: :ok
  def socket_start_read(socket, listener, token) do
    Rezmq.Native.socket_start_read(socket, listener, token)
  end

  @spec socket_abort_read(Rezmq.Native.native_socket()) :: :ok
  def socket_abort_read(socket) do
    Rezmq.Native.socket_abort_read(socket)
  end

  @spec socket_write(Rezmq.Native.native_socket(), list(binary())) :: :ok | {:error, term()}
  def socket_write(socket, data) do
    data = :erlang.term_to_binary(data)

    case Rezmq.Native.socket_write_encoded_fast(socket, data) do
      :ok ->
        :ok

      x when x == :error or x == :would_block ->
        Rezmq.Native.socket_write_encoded(socket, data)
    end
  end
end
