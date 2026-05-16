import { invoke } from "@tauri-apps/api/tauri";
import { buildFullPath } from "../pruneData";
interface ParentFolderProps {
  focusedDirectory: D3HierarchyDiskItem;
  d3Chart: any;
}
export const ParentFolder = ({
  focusedDirectory,
  d3Chart,
}: ParentFolderProps) => {
  // Computed at render time so the value is current even if window.OS_TYPE
  // was populated asynchronously after the module loaded.
  const mul = window.OS_TYPE === "Windows_NT" ? 1024 : 1000;
  return (
    <div
      className="bg-gray-800 p-2 text-white flex justify-between rounded-md cursor-pointer"
      onContextMenu={(e) => {
        e.preventDefault();
        invoke("show_in_folder", { path: buildFullPath(focusedDirectory) });
      }}
      onClick={() => {
        if (focusedDirectory.parent)
          d3Chart.current.backToParent(focusedDirectory.parent);
      }}
    >
      <div className=""></div>
      <div className="truncate pr-6 flex-1 text-xs">
        {focusedDirectory &&
          buildFullPath(focusedDirectory)
            .replace(/\\\//g, "/")
            .replace(/\\/g, "/")}
      </div>
      <div className="text-xs">
        {focusedDirectory &&
          (focusedDirectory.data.value! / mul / mul / mul).toFixed(2)}{" "}
        GB
      </div>
    </div>
  );
};
