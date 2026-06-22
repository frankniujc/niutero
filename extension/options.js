import { getConfig, setConfig, DEFAULT_PORT } from "./lib/config.js";
import { health } from "./lib/connector.js";

const $ = (id) => document.getElementById(id);

async function load() {
  const c = await getConfig();
  $("port").value = c.port;
  $("token").value = c.token;
}

async function save() {
  await setConfig({ port: $("port").value, token: $("token").value });
  flash("Saved.", "good");
}

async function test() {
  await save();
  flash("Checking…", "");
  const c = await getConfig();
  const up = await health(c);
  if (up) {
    flash(`Connector is up on 127.0.0.1:${c.port}.`, "good");
  } else {
    flash(
      `No connector on 127.0.0.1:${c.port}. Start it with: niutero-cli connector <vault>`,
      "bad",
    );
  }
}

function flash(msg, cls) {
  const s = $("status");
  s.textContent = msg;
  s.className = "status " + (cls || "");
}

$("save").addEventListener("click", save);
$("test").addEventListener("click", test);
$("reset").addEventListener("click", () => {
  $("port").value = DEFAULT_PORT;
});

load();
