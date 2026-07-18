import type { ComponentProps, SVGProps } from "react";
import {
  AlertTriangle,
  ArrowLeft,
  Check,
  CheckCircle2,
  ChevronLeft,
  ChevronRight,
  CircleOff,
  Cloud,
  CloudAlert,
  CloudCog,
  CloudDownload,
  CloudOff,
  CloudUpload,
  Copy,
  Download,
  Eye,
  FolderPlus,
  FolderSearch,
  Gamepad2,
  Gauge,
  Grid2X2,
  HardDrive,
  HardDriveDownload,
  Home,
  Layers3,
  LibraryBig,
  List,
  LoaderCircle,
  Maximize2,
  Menu,
  MonitorPlay,
  MoreVertical,
  NotebookPen,
  Pin,
  Play,
  Plus,
  RefreshCw,
  RotateCcw,
  Save,
  Search,
  Settings,
  ShieldCheck,
  SlidersHorizontal,
  Sparkles,
  SquareStack,
  Tags,
  Trash2,
  Upload,
  Video,
  Volume2,
  VolumeX,
  WandSparkles,
  X,
  type LucideIcon
} from "lucide-react";

export type IconName =
  | "add" | "archive" | "back" | "cancel" | "check" | "cleanup" | "clip"
  | "close" | "collection" | "copy" | "download" | "error" | "filter" | "game"
  | "grid" | "home" | "library" | "list" | "local" | "maximize" | "menu" | "more"
  | "notes" | "play" | "protected" | "recording" | "recorder" | "refresh" | "repair"
  | "restore" | "save" | "search" | "settings" | "storage" | "success" | "tags"
  | "trash" | "update" | "upload" | "volume" | "volumeOff" | "warning"
  | "nudgeLeft" | "nudgeRight" | "preparing";

const icons: Record<IconName, LucideIcon> = {
  add: Plus,
  archive: ShieldCheck,
  back: ArrowLeft,
  cancel: CircleOff,
  check: Check,
  cleanup: Trash2,
  clip: Sparkles,
  close: X,
  collection: Layers3,
  copy: Copy,
  download: Download,
  error: CloudAlert,
  filter: SlidersHorizontal,
  game: Gamepad2,
  grid: Grid2X2,
  home: Home,
  library: LibraryBig,
  list: List,
  local: HardDrive,
  maximize: Maximize2,
  menu: Menu,
  more: MoreVertical,
  notes: NotebookPen,
  play: Play,
  protected: Pin,
  recording: Video,
  recorder: MonitorPlay,
  refresh: RefreshCw,
  repair: WandSparkles,
  restore: HardDriveDownload,
  save: Save,
  search: Search,
  settings: Settings,
  storage: Gauge,
  success: CheckCircle2,
  tags: Tags,
  trash: Trash2,
  update: RotateCcw,
  upload: Upload,
  volume: Volume2,
  volumeOff: VolumeX,
  warning: AlertTriangle,
  nudgeLeft: ChevronLeft,
  nudgeRight: ChevronRight,
  preparing: LoaderCircle
};

export function AppIcon({ name, size = 18, className, ...props }: {
  name: IconName;
  size?: number;
} & Omit<ComponentProps<LucideIcon>, "name" | "size">) {
  const Icon = icons[name];
  return <Icon aria-hidden="true" className={className} size={size} strokeWidth={1.8} {...props} />;
}

export function FrameStackMark({ size = 28, className, ...props }: {
  size?: number;
} & SVGProps<SVGSVGElement>) {
  return (
    <svg aria-hidden="true" className={className} width={size} height={size} viewBox="0 0 24 24" fill="none" {...props}>
      <path d="M7.7 3.2h10.1c.7 0 1.2.5 1.2 1.2v.8L6.4 6.7v-1c0-1.3.5-2.2 1.3-2.5Z" fill="currentColor" opacity=".72" />
      <path d="m5.4 6.3 12.1-1.4c1.1-.1 2 .8 2 1.9v2.1H8.7c-1 0-1.8.8-1.8 1.8v4.7H5.5c-.9 0-1.6-.7-1.6-1.6V8.1c0-.9.6-1.7 1.5-1.8Z" fill="var(--raw, #7dd3fc)" />
      <path fillRule="evenodd" d="M8.2 8.4h9.9c.9 0 1.7.7 1.7 1.7v5.4c0 .9-.8 1.7-1.7 1.7h-2.4L12.5 14h3.2c.7 0 1.2-.6 1.2-1.2v-.6c0-.7-.5-1.2-1.2-1.2h-5.2c-.7 0-1.2.5-1.2 1.2v5H7.2c-.9 0-1.7-.8-1.7-1.7v-4.4c0-1.5 1.2-2.7 2.7-2.7Z" fill="currentColor" />
    </svg>
  );
}

export type AvailabilityState =
  | "localOnly" | "queued" | "preparing" | "uploading" | "waitingForQuota"
  | "processingRemote" | "archivedVerified" | "remoteOnly" | "uploadFailed"
  | "remoteMissing" | "multipleRemoteCopies";

const availability: Record<AvailabilityState, { label: string; icon: LucideIcon; tone: string }> = {
  localOnly: { label: "Local only", icon: HardDrive, tone: "local" },
  queued: { label: "Queued for upload", icon: CloudUpload, tone: "queued" },
  preparing: { label: "Preparing", icon: CloudCog, tone: "progress" },
  uploading: { label: "Uploading", icon: CloudUpload, tone: "progress" },
  waitingForQuota: { label: "Waiting for quota", icon: CloudOff, tone: "warning" },
  processingRemote: { label: "Processing remotely", icon: CloudCog, tone: "progress" },
  archivedVerified: { label: "Archived and verified", icon: ShieldCheck, tone: "success" },
  remoteOnly: { label: "Remote only", icon: Cloud, tone: "remote" },
  uploadFailed: { label: "Upload failed", icon: CloudAlert, tone: "danger" },
  remoteMissing: { label: "Remote copy missing", icon: CloudOff, tone: "danger" },
  multipleRemoteCopies: { label: "Multiple remote copies", icon: SquareStack, tone: "remote" }
};

export function AvailabilityIcon({ state, size = 18, showLabel = false, className }: {
  state: AvailabilityState;
  size?: number;
  showLabel?: boolean;
  className?: string;
}) {
  const definition = availability[state];
  const Icon = definition.icon;
  return (
    <span className={`availability-icon ${definition.tone} ${className ?? ""}`} title={definition.label} aria-label={definition.label}>
      <Icon aria-hidden="true" size={size} strokeWidth={1.8} />
      {showLabel ? <span>{definition.label}</span> : null}
    </span>
  );
}

export const availabilityLabel = (state: AvailabilityState) => availability[state].label;

// Kept here so feature code never imports a second icon vocabulary for these states.
export const archiveActionIcons = {
  upload: CloudUpload,
  restore: CloudDownload,
  openRemote: Eye,
  keepLocal: Pin,
  removeRemote: CloudOff,
  recorder: MonitorPlay,
  browse: FolderSearch,
  classify: Gamepad2,
  collection: Layers3,
  media: SquareStack,
  magic: Sparkles,
  folder: FolderPlus
} as const;
