CREATE OR REPLACE PROCEDURE add_guild_food(
    food_id IN INT, 
    lvl IN INT, 
    g_id IN INT, 
    requested_hours IN INT
)
LANGUAGE plpgsql
AS $$
DECLARE
    existing_food_id INT;
BEGIN
    -- Check if food with same ID, level, and guild exists
    SELECT id INTO existing_food_id
    FROM guild_meals
    WHERE guild_id = g_id AND meal_id = food_id AND level = lvl;

    IF FOUND THEN
        -- EXTEND: Add hours to existing expiration
        UPDATE guild_meals
        SET expires_in = expires_in + (requested_hours - 1) * INTERVAL '1 hour'
        WHERE guild_id = g_id AND meal_id = food_id AND level = lvl;
    ELSE
        -- INSERT: Create new food entry
        INSERT INTO guild_meals (guild_id, meal_id, level, expires_in)
        VALUES (g_id, food_id, lvl, CURRENT_TIMESTAMP + (requested_hours - 1) * INTERVAL '1 hour');
    END IF;
END;
$$;