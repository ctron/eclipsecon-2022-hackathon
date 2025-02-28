#!/usr/bin/env python3
import blemesh
try:
  from gi.repository import GLib
except ImportError:
  import glib as GLib
from dbus.mainloop.glib import DBusGMainLoop
import dbus
import dbus.service
import dbus.exceptions
import sys
import os

def main():
	blemesh.configure_logging("gateway")

	DBusGMainLoop(set_as_default=True)
	blemesh.bus = dbus.SystemBus()

	blemesh.mesh_net = dbus.Interface(blemesh.bus.get_object(blemesh.MESH_SERVICE_NAME,
						"/org/bluez/mesh"),
						blemesh.MESH_NETWORK_IFACE)

	blemesh.mesh_net.connect_to_signal('InterfacesRemoved', blemesh.interfaces_removed_cb)

	blemesh.app = blemesh.Application(blemesh.bus, '/gateway/application')

	# Provisioning agent
	blemesh.app.set_agent(blemesh.Agent(blemesh.bus))

	first_ele = blemesh.Element(blemesh.bus, '/gateway/ele', 0x00, 0x0100)
	first_ele.add_model(blemesh.Model(0x1001))
	first_ele.add_model(blemesh.Model(0x100D))
	first_ele.add_model(blemesh.Model(0x1102))

	second_ele = blemesh.Element(blemesh.bus, '/gateway/ele', 0x01, 0x010D)
	second_ele.add_model(blemesh.Model(0x1000))

	third_ele = blemesh.Element(blemesh.bus, '/gateway/ele', 0x02, 0x010E)
	third_ele.add_model(blemesh.Model(0x1000))

	blemesh.app.add_element(first_ele)
	blemesh.app.add_element(second_ele)
	blemesh.app.add_element(third_ele)

	blemesh.mainloop = GLib.MainLoop()

	blemesh.join()
	blemesh.mainloop.run()


if __name__ == '__main__':
	main()
