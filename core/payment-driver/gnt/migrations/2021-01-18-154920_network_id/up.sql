ALTER TABLE gnt_driver_payment ADD COLUMN network INTEGER NOT NULL DEFAULT 4;  -- 4 is rinkeby's network ID

ALTER TABLE gnt_driver_transaction ADD COLUMN network INTEGER NOT NULL DEFAULT 4;  -- 4 is rinkeby's network ID
