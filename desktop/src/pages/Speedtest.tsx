import Importer from "./speedtest/Importer";
import NodeList from "./speedtest/NodeList";
import ControlBar from "./speedtest/ControlBar";
import ResultsPanel from "./speedtest/ResultsPanel";
import NetworkInfo from "../components/NetworkInfo";

export default function Speedtest() {
  return (
    <div className="flex flex-col gap-4">
      <div className="grid grid-cols-1 xl:grid-cols-2 gap-4">
        <Importer />
        <ControlBar />
      </div>
      <NodeList />
      <ResultsPanel />
      <NetworkInfo />
    </div>
  );
}
