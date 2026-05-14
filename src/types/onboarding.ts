import type { LessonPlan } from "./manifest";

export interface ChatMessage {
  role: "user" | "assistant";
  content: string;
}

export interface GeneratedPlan {
  summary: string;
  description: string;
  readingLevel: string;
  priorKnowledge: string;
  lessonPlan: LessonPlan;
}
