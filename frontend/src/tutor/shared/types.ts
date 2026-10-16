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

/** 是非題作答值為 boolean（true = 對），選擇題為 1-based 題號 */
export type ChoiceValue = boolean | number;

export interface RevealInfo {
  answer: string;
  explanation: string;
  /** 是非/選擇題的正解 */
  choice?: ChoiceValue;
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
  /** 答錯時先回對錯，老師的提示要另外呼叫 /hint 取得；say 是取不到時的備用提示 */
  hintPending?: boolean;
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
