-- Hierarchical card relation: a child card is part of a parent card.
ALTER TABLE card_relations
    DROP CONSTRAINT IF EXISTS card_relations_relation_type_check;

ALTER TABLE card_relations
    ADD CONSTRAINT card_relations_relation_type_check
    CHECK (relation_type IN ('blocks', 'depends_on', 'duplicate', 'related', 'part_of'));
