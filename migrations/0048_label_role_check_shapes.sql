ALTER TABLE labels
    DROP CONSTRAINT IF EXISTS labels_icon_shape_check;

ALTER TABLE labels
    ADD CONSTRAINT labels_icon_shape_check
    CHECK (icon_shape IN ('circle', 'square', 'diamond', 'star', 'triangle', 'hexagon', 'bolt', 'flag', 'check', 'cross'));

ALTER TABLE profile_roles
    DROP CONSTRAINT IF EXISTS profile_roles_icon_shape_check;

ALTER TABLE profile_roles
    ADD CONSTRAINT profile_roles_icon_shape_check
    CHECK (icon_shape IN ('circle', 'square', 'diamond', 'star', 'triangle', 'hexagon', 'bolt', 'flag', 'check', 'cross'));
