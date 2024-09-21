import {
  createEffect,
  createResource,
  createSignal,
  Match,
  on,
  Switch,
  type Component,
} from "solid-js";

import logo from "./logo.svg";
import styles from "./App.module.css";
import { createWS, createWSState } from "@solid-primitives/websocket";
import { ClientMessage, GameState } from "./schemas/types";

const App: Component = () => {
  const [state, setState] = createSignal<GameState>();
  const hostaddr = window.location.pathname.substring(1);
  const hostConn = createWS("ws://" + hostaddr);
  const connState = createWSState(hostConn);
  const states = ["Connecting", "Connected", "Disconnecting", "Disconnected"];
  hostConn.addEventListener("message", (e) => setState(JSON.parse(e.data)));

  function Send(m: ClientMessage) {
    hostConn.send(JSON.stringify(m));
  }

  return (
    <div>
      <Switch fallback={<>Disconnected</>}>
        <Match when={state()?.variant == "SongSelect"}>
          <input
            type="text"
            onchange={(x) =>
              Send({ variant: "SetSearch", v: x.currentTarget.value })
            }
          ></input>
        </Match>
        <Match when={state()?.variant == "None"}>
          <div></div>
        </Match>
        <Match when={state()?.variant == "TitleScreen"}>
          <button onclick={() => Send({ variant: "Start" })}>Start</button>
        </Match>
      </Switch>
    </div>
  );
};

export default App;
