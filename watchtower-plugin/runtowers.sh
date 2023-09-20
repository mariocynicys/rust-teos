###########################
printf "Press enter to start teosd-rusqlite "
read
~/teosd-rusqlite >/dev/null 2>/dev/null &
printf "Press enter to stop teosd-rusqlite "
read
teos-cli stop
sleep 1


###########################
printf "Press enter to start teosd-1lite "
read
~/teosd-1lite --databaseurl "sqlite:///home/mario/sqlx.db?mode=rwc" >/dev/null 2>/dev/null &
printf "Press enter to stop teosd-1lite "
read
teos-cli stop
sleep 1


###########################
printf "Press enter to start teosd-sqlx "
read
~/teosd-sqlx --databaseurl "sqlite:///home/mario/sqlx.db?mode=rwc" >/dev/null 2>/dev/null &
printf "Press enter to stop teosd-sqlx "
read
teos-cli stop
sleep 1


###########################
printf "Press enter to start teosd-gres "
read
~/teosd-gres --databaseurl "postgres://user:pass@localhost/teos" >/dev/null 2>/dev/null &
printf "Press enter to stop teosd-gres "
read
teos-cli stop
sleep 1
