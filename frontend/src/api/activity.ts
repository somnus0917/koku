import { request } from "./client";
import type { ActivityEvent } from "../types";

export const loadActivity = (limit = 80) => request<ActivityEvent[]>(`/api/activity?limit=${limit}`);
