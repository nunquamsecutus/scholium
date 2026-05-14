export type ChapterStatus = "planned" | "generating" | "generated";

export interface Chapter {
  id: string;
  title: string;
  description: string;
  file: string;
  status: ChapterStatus;
}

export interface LessonPlan {
  summary: string;
  chapters: Chapter[];
}

export interface Metadata {
  title: string;
  topic: string;
  created: string;
  modified: string;
  readingLevel?: string;
  priorKnowledge?: string;
}

export interface Manifest {
  version: number;
  metadata: Metadata;
  lessonPlan: LessonPlan;
}
