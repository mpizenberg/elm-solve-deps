let depsProvider = require("./dependency-provider-offline.js");
let wasm = require("elm-solve-deps-wasm");
wasm.init();

let elm_json_config = `
{
    "type": "package",
    "name": "ianmackenzie/elm-units-interval",
    "summary": "Version of elm-interval based on elm-units",
    "license": "MPL-2.0",
    "version": "2.3.0",
    "exposed-modules": [
        "Quantity.Interval",
        "Temperature.Interval",
        "Angle.Interval"
    ],
    "elm-version": "0.19.0 <= v < 0.20.0",
    "dependencies": {
        "elm/core": "1.0.0 <= v < 2.0.0",
        "elm/random": "1.0.0 <= v < 2.0.0",
        "ianmackenzie/elm-float-extra": "1.1.0 <= v < 2.0.0",
        "ianmackenzie/elm-units": "2.7.0 <= v < 3.0.0"
    },
    "test-dependencies": {
        "elm-explorations/test": "1.1.0 <= v < 2.0.0"
    }
}
`;
let solution = wasm.solve_deps(
  elm_json_config,
  false,
  {},
  depsProvider.fetchElmJson,
  depsProvider.listAvailableVersions
);

console.log(JSON.parse(solution));
