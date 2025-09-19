CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

CREATE TABLE courses (
  id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
  title TEXT NOT NULL,
  org_identifier TEXT,
  launch_href TEXT NOT NULL,
  base_path TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE scos (
  id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
  course_id UUID NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
  identifier TEXT NOT NULL,
  launch_href TEXT NOT NULL,
  parameters TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE attempts (
  id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
  course_id UUID NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
  learner_id TEXT NOT NULL,
  sco_id UUID REFERENCES scos(id),
  status TEXT NOT NULL DEFAULT 'not_started',
  started_at TIMESTAMPTZ,
  finished_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE cmi_values (
  attempt_id UUID NOT NULL REFERENCES attempts(id) ON DELETE CASCADE,
  element TEXT NOT NULL,
  value TEXT,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (attempt_id, element)
);

CREATE INDEX idx_attempts_course_learner ON attempts(course_id, learner_id);

