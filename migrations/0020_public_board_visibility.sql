ALTER TYPE board_visibility ADD VALUE IF NOT EXISTS 'public';

UPDATE boards SET visibility = 'private' WHERE visibility = 'workspace';
