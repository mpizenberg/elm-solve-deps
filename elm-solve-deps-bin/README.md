# Dependency solving for the elm ecosystem

This `elm-solve-deps` program provides a dedicated dependency solver as a CLI executable for the elm ecosystem.
It is based on the [elm-solve-deps][elm-solve-deps] crate, giving the same capabilities in the form of a Rust library.

[pubgrub]: https://github.com/pubgrub-rs/pubgrub
[elm-solve-deps]: https://github.com/mpizenberg/elm-solve-deps/tree/master/elm-solve-deps-lib

The main objective of dependency solving is to start from
a set of dependency constraints, provided for example by the `elm.json` of a package:

```json
{
  ...,
  "dependencies": {
    "elm/core": "1.0.2 <= v < 2.0.0",
    "elm/http": "2.0.0 <= v < 3.0.0",
    "elm/json": "1.1.2 <= v < 2.0.0"
  },
  "test-dependencies": {
    "elm-explorations/test": "1.2.0 <= v < 2.0.0"
  }
}
```

And then find a set of package versions satisfying these constraints.
In general we also want some prioritization, such as picking the newest versions compatible.
In this case and at this date, without considering the test dependencies, the newest solution is:

```json
{
  "direct": {
    "elm/core": "1.0.5",
    "elm/http": "2.0.0",
    "elm/json": "1.1.3"
  },
  "indirect": {
    "elm/bytes": "1.0.8",
    "elm/file": "1.0.5",
    "elm/time": "1.0.0"
  }
}
```

And if we also consider the tests dependencies, we get instead:

```json
{
  "direct": {
    "elm/core": "1.0.5",
    "elm/http": "2.0.0",
    "elm/json": "1.1.3",
    "elm-explorations/test": "1.2.2"
  },
  "indirect": {
    "elm/bytes": "1.0.8",
    "elm/file": "1.0.5",
    "elm/html": "1.0.0",
    "elm/random": "1.0.0",
    "elm/time": "1.0.0",
    "elm/virtual-dom": "1.0.2"
  }
}
```

## Usage of this `elm-solve-deps` CLI.

Here is the help message (maybe outdated) of the CLI program showing most of its capabilities.
You can get an up-to-date version of this output by running `elm-solve-deps --help`.

```txt
elm-solve-deps

Solve dependencies of an Elm project or published package.
By default, try in offline mode first
and switch to online mode if that fails.

USAGE:
    elm-solve-deps [FLAGS...] [author/package@version]
    For example:
        elm-solve-deps
        elm-solve-deps --help
        elm-solve-deps --offline
        elm-solve-deps ianmackenzie/elm-3d-scene@1.0.1
        elm-solve-deps --offline jxxcarlson/elm-tar@4.0.0
        elm-solve-deps --online-newest w0rm/elm-physics@5.1.1
        elm-solve-deps --online-oldest lucamug/style-framework@1.1.0
        elm-solve-deps --test
        elm-solve-deps --extra "elm/json: 1.1.3 <= v < 2.0.0"

FLAGS:
    --help                 Print this message and exit
    --offline              No network request, use only installed packages
    --online-newest        Use the newest compatible version
    --online-oldest        Use the oldest compatible version
    --test                 Solve with both normal and test dependencies
    --extra "author/package: constraint"
                           Additional package version constraint
                           Need one --extra per additional constraint
                           MUST be placed before an eventual package to solve
```
