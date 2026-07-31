import { create } from "zustand";
import type { Liveness, ManualStatus, Session } from "../types";

export interface Filters {
  search: string;
  liveness: Liveness | null;
  status: ManualStatus | null;
}

interface FilterStore extends Filters {
  setSearch: (s: string) => void;
  setLiveness: (l: Liveness | null) => void;
  setStatus: (s: ManualStatus | null) => void;
}

export const useFilters = create<FilterStore>((set) => ({
  search: "",
  liveness: null,
  status: null,
  setSearch: (search) => set({ search }),
  setLiveness: (liveness) => set({ liveness }),
  setStatus: (status) => set({ status }),
}));

export function applyFilters(sessions: Session[], f: Filters): Session[] {
  const needle = f.search.trim().toLowerCase();
  return sessions.filter((s) => {
    if (f.liveness && s.liveness !== f.liveness) return false;
    if (f.status && s.annotation.status !== f.status) return false;
    if (!needle) return true;
    const haystack = [
      s.aiTitle ?? "",
      s.lastPrompt ?? "",
      s.projectLabel,
      s.gitBranch ?? "",
      s.annotation.notes,
      s.annotation.displayName ?? "",
    ]
      .join(" ")
      .toLowerCase();
    return haystack.includes(needle);
  });
}
