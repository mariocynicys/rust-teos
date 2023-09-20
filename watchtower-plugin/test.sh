cd $(dirname $0)
users=$1
appointments=$2

# cargo build --target x86_64-unknown-linux-musl --release --bin stress
# cp ../target/x86_64-unknown-linux-musl/release/stress .

cargo build --release --bin stress
cp ../target/release/stress .


###########################
~/teosd-rusqlite >/dev/null 2>/dev/null &
echo "rusqlite sync driver:"
sleep 1
./stress $users $appointments
teos-cli stop
sleep 1


###########################
~/teosd-1lite --databaseurl "sqlite:///home/mario/sqlx.db?mode=rwc" >/dev/null 2>/dev/null &
echo "Sqlx sqlite pool of 1 connection:"
sleep 1
./stress $users $appointments
teos-cli stop
sleep 1


###########################
~/teosd-sqlx --databaseurl "sqlite:///home/mario/sqlx.db?mode=rwc" >/dev/null 2>/dev/null &
echo "Sqlx sqlite pool of 20 connections:"
sleep 1
./stress $users $appointments
teos-cli stop
sleep 1


###########################
~/teosd-gres --databaseurl "postgres://user:pass@localhost/teos" >/dev/null 2>/dev/null &
echo "Sqlx postgresql:"
sleep 1
./stress $users $appointments
teos-cli stop
sleep 1
