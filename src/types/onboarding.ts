import type { LessonPlan } from "./manifest";

export type ReadingLevel = "child" | "teen" | "adult" | "academic";

export interface ChatMessage {
  role: "user" | "assistant";
  content: string;
}

export interface GeneratedPlan {
  summary: string;
  description: string;
  priorKnowledge: string;
  lessonPlan: LessonPlan;
}
