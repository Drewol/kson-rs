import {
  createEffect,
  createResource,
  createSignal,
  Match,
  on,
  Show,
  Switch,
  type Component,
} from "solid-js";

import logo from "./logo.svg";
import styles from "./App.module.css";
import {
  createReconnectingWS,
  createWS,
  createWSState,
} from "@solid-primitives/websocket";
import { ClientEvent, GameState } from "./schemas/types";
import { SongSelect } from "./SongSelect";

const App: Component = () => {
  const [state, setState] = createSignal<GameState>();
  const hostaddr = window.location.pathname.substring(1);
  const hostConn = createReconnectingWS("ws://" + hostaddr);
  const connState = createWSState(hostConn);
  const states = ["Connecting", "Connected", "Disconnecting", "Disconnected"];
  hostConn.addEventListener("message", (e) => {
    const current = JSON.stringify(state());
    const newState: GameState = JSON.parse(e.data);

    if (current != JSON.stringify(newState)) {
      setState(newState);
    }
  });

  function Send(m: ClientEvent) {
    hostConn.send(JSON.stringify(m));
  }

  return (
    <div class="bg-slate-900 h-screen w-screen p-5 text-zinc-100 max-h-screen flex flex-col">
      <Show
        when={connState() == 1}
        fallback={<div class="text-3xl text-amber-500">Disconnected</div>}
      >
        <Switch fallback={<>Disconnected</>}>
          <Match when={state()?.variant == "SongSelect"}>
            <SongSelect state={state as any} send={Send}></SongSelect>
          </Match>
          <Match when={state()?.variant == "None"}>
            <div></div>
          </Match>
          <Match when={state()?.variant == "TitleScreen"}>
            <button onclick={() => Send({ variant: "Start" })}>Start</button>
          </Match>
        </Switch>
      </Show>
    </div>
  );
};

export default App;
