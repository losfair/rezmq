defmodule Rezmq.Native do
  use Rustler, otp_app: :rezmq, crate: "rezmq_native"

  @type native_context() :: any()
  @type native_worker() :: any()
  @type native_socket() :: any()

  @spec context_new() :: native_context()
  def context_new(), do: :erlang.nif_error(:nif_not_loaded)

  @spec worker_start(native_context()) :: native_worker()
  def worker_start(_context), do: :erlang.nif_error(:nif_not_loaded)

  @spec worker_stop(native_worker()) :: :ok
  def worker_stop(_worker), do: :erlang.nif_error(:nif_not_loaded)

  @spec socket_create(native_worker(), integer()) :: {:ok, native_socket()} | {:error, integer()}
  def socket_create(_worker, _socket_type), do: :erlang.nif_error(:nif_not_loaded)

  @spec socket_destroy(native_socket()) :: :ok
  def socket_destroy(_socket), do: :erlang.nif_error(:nif_not_loaded)

  @spec socket_setsockopt(native_socket(), integer(), binary()) :: :ok | {:error, integer()}
  def socket_setsockopt(_socket, _option_name, _option_value),
    do: :erlang.nif_error(:nif_not_loaded)

  @spec socket_bind(native_socket(), binary()) :: :ok | {:error, integer()}
  def socket_bind(_socket, _address), do: :erlang.nif_error(:nif_not_loaded)

  @spec socket_connect(native_socket(), binary()) :: :ok | {:error, integer()}
  def socket_connect(_socket, _address), do: :erlang.nif_error(:nif_not_loaded)

  @spec socket_start_read(native_socket(), pid(), reference(), tuple() | nil) :: :ok
  def socket_start_read(_socket, _listener, _token, _metadata_properties),
    do: :erlang.nif_error(:nif_not_loaded)

  @spec socket_abort_read(native_socket()) :: :ok
  def socket_abort_read(_socket), do: :erlang.nif_error(:nif_not_loaded)

  @spec socket_write_encoded(native_socket(), binary()) :: :ok | {:error, term()}
  def socket_write_encoded(_socket, _data), do: :erlang.nif_error(:nif_not_loaded)

  @spec socket_write_encoded_fast(native_socket(), binary()) :: :ok | :error | :would_block
  def socket_write_encoded_fast(_socket, _data), do: :erlang.nif_error(:nif_not_loaded)
end
