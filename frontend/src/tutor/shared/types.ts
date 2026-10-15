export interface QuestionView {
  id: string;
  qtype: string;
  inputMode: 'choice' | 'char_grid' | 'free_canvas' | 'image_canvas' | 'text';
  section: string;
  stem: string;
  options: string[];
  charCount?: number;
  imageUrl?: string;
  bookPage?: number;
  bookWhere?: string;
}

export interface BookRef {
  page: number;
  where: string;
}

export interface DialogTurnView {
  role: 'tutor' | 'student';
  content: string;
  bookRef?: BookRef;
}

export interface SessionView {
  sessionId: string;
  mode: string;
  index: number;
  total: number;
  qState: 'asking' | 'hinting' | 'revealed' | 'explained';
  question?: QuestionView;
  empty?: boolean;
  message?: string;
  dialog: DialogTurnView[];
}

export interface RevealInfo {
  answer: string;
  explanation: string;
}

export interface TutorReply {
  verdict: 'correct' | 'partial' | 'incorrect' | 'unreadable' | 'none';
  say: string;
  action: string;
  bookRef?: BookRef;
  recognizedText?: string;
  followupQuestion?: string;
  qState: SessionView['qState'];
  reveal?: RevealInfo;
  canNext: boolean;
  selfGradeAvailable?: boolean;
}

export interface StudentItem {
  id: number;
  name: string;
  grade: number;
}

export interface SubjectCatalogItem {
  id: string;
  name: string;
  bookTitle: string;
  units: UnitCatalogItem[];
}

export interface UnitCatalogItem {
  id: string;
  title: string;
  total: number;
  mastered: number;
}
