defmodule Rezmq.MixProject do
  use Mix.Project

  @source_url "https://github.com/losfair/rezmq"

  def project do
    [
      app: :rezmq,
      version: "0.1.0",
      description: "Fast ZeroMQ bindings for Elixir",
      elixir: "~> 1.17",
      start_permanent: Mix.env() == :prod,
      deps: deps(),
      package: [
        maintainers: ["Heyang Zhou"],
        licenses: ["MIT"],
        links: %{"GitHub" => @source_url}
      ]
    ]
  end

  # Run "mix help compile.app" to learn about applications.
  def application do
    [
      extra_applications: [:logger]
    ]
  end

  # Run "mix help deps" to learn about dependencies.
  defp deps do
    [
      {:rustler, "~> 0.35.0"},
      {:benchee, "~> 1.0", only: :dev},
      {:ex_doc, ">= 0.0.0", only: :dev, runtime: false}
      # {:dep_from_hexpm, "~> 0.3.0"},
      # {:dep_from_git, git: "https://github.com/elixir-lang/my_dep.git", tag: "0.1.0"}
    ]
  end
end
