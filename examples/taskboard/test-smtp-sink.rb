#!/usr/bin/env ruby
# Local-only SMTP fault-injection sink. It never logs message contents.
require "socket"

STDOUT.sync = true
server = TCPServer.new("0.0.0.0", 2525)
accepted = 0
lock = Mutex.new
trap("TERM") { server.close; exit }
puts "smtp sink ready"

loop do
  client = server.accept
  Thread.new(client) do |socket|
    socket.write("220 local SMTP\r\n")
    data = false
    while (line = socket.gets)
      if data
        next unless line == ".\r\n"

        lock.synchronize do
          accepted += 1
          puts "accepted message #{accepted}"
        end
        socket.write("250 accepted\r\n")
        data = false
      elsif line.start_with?("EHLO", "HELO")
        socket.write("250 local\r\n")
      elsif line.start_with?("MAIL FROM:", "RCPT TO:")
        socket.write("250 OK\r\n")
      elsif line.start_with?("DATA")
        socket.write("354 go\r\n")
        data = true
      elsif line.start_with?("QUIT")
        socket.write("221 bye\r\n")
        break
      else
        socket.write("500 unsupported\r\n")
      end
    end
  ensure
    socket.close
  end
end
